use bitintr::{Pdep, Pext};
use itertools::Itertools;
use std::fmt;
use std::fmt::Write as _;

use crate::state_info::{PieceGrid, StateInfo};
use crate::{Bitboard, Color, Hand, Move, MoveError, Piece, PieceType, SfenError, Square};

mod test;

/// MoveRecord stores information necessary to undo the move.
#[derive(Debug)]
pub enum MoveRecord {
    Normal {
        from: Square,
        to: Square,
        placed: Piece,
        captured: Option<Piece>,
        promoted: bool,
    },
    Drop {
        to: Square,
        piece: Piece,
    },
}

impl MoveRecord {
    /// Converts the move into SFEN formatted string.
    pub fn to_sfen(&self) -> String {
        match *self {
            MoveRecord::Normal { from, to, promoted, .. } => {
                format!("{}{}{}", from, to, if promoted { "+" } else { "" })
            }
            MoveRecord::Drop {
                to,
                piece: Piece { piece_type, .. },
            } => format!("{}*{}", piece_type.to_string().to_uppercase(), to),
        }
    }
}

impl PartialEq<Move> for MoveRecord {
    fn eq(&self, other: &Move) -> bool {
        match (self, other) {
            (
                &MoveRecord::Normal {
                    from: f1,
                    to: t1,
                    promoted,
                    ..
                },
                &Move::Normal {
                    from: f2,
                    to: t2,
                    promote,
                },
            ) => f1 == f2 && t1 == t2 && promote == promoted,
            (&MoveRecord::Drop { to: t1, piece, .. }, &Move::Drop { to: t2, piece_type }) => {
                t1 == t2 && piece.piece_type == piece_type
            }
            _ => false,
        }
    }
}

// A compact representation of Position
// Layout (bit index):
// bits[0..63]   : occupied_low (63 bits)
// bit 63        : black_king_index < white_king_index
//
// bits[64..81]  : occupied_high (18 bits)
// bits[81..89]  : is_silver (8 bits)
// bits[89..128] : is_black (38 bits)
//
// bits[128..168]: is_pawn (40 bits)
// bits[168..182]: is_silver_or_gold (14 bits)
// bits[182..188]: is_bishop_or_rook (6 bits)
// bits[188..192]: is_bishop (4 bits)
//
// bits[192..226]: is_promoted (34 bits)
// bits[226..248]: is_lance_or_knight (22 bits)
// bits[248..256]: is_lance (8 bits)
//
// Total: 256 bits = 4 u64s
//
// Layout of color bits:
// bits[0..(N-1)]: color of pieces on board except kings (N = occupied_bb.count() - 2)
// bits[(N)..]: color of pieces in hand (rest bits)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedStateInfo([u64; 4]);

impl PackedStateInfo {
    fn pack_by_occupied(state: &StateInfo) -> [u64; 14] {
        PieceType::iter()
            .map(|pt| state.type_bb[pt.index()].pext_u64(&state.occupied_bb))
            .collect_vec()
            .try_into()
            .unwrap()
    }

    // Pack hand pieces' colors into bits.
    // Each piece type uses (num_black + num_white) bits, where
    // num_black bits are set to 1, and num_white bits are set to 0.
    // piece types are packed in the order of
    // Pawn, Lance, Knight, Silver, Gold, Bishop, Rook (from LSB to MSB).
    fn packed_hand_colors(state: &StateInfo) -> u64 {
        [
            PieceType::Pawn,
            PieceType::Lance,
            PieceType::Knight,
            PieceType::Silver,
            PieceType::Gold,
            PieceType::Bishop,
            PieceType::Rook,
        ]
        .iter()
        .rev()
        .fold(0u64, |mut accum, &pt| {
            let num_black = state.hand.get(Piece {
                piece_type: pt,
                color: Color::Black,
            }) as u64;
            let num_white = state.hand.get(Piece {
                piece_type: pt,
                color: Color::White,
            }) as u64;
            accum <<= num_black + num_white;
            accum |= (1u64 << num_black) - 1;
            accum
        })
    }

    fn type_bb_to_piece_grid(bbs: &[Bitboard; 14], color_bb: &Bitboard) -> PieceGrid {
        let mut board = PieceGrid([None; 81]);
        for (pt, bb) in PieceType::iter().zip(bbs.iter()) {
            let mut black_bb = bb & color_bb;
            let mut white_bb = bb & &!color_bb;
            while black_bb.is_any() {
                let sq = black_bb.pop();
                board.set(
                    sq,
                    Some(Piece {
                        piece_type: pt,
                        color: Color::Black,
                    }),
                );
            }
            while white_bb.is_any() {
                let sq = white_bb.pop();
                board.set(
                    sq,
                    Some(Piece {
                        piece_type: pt,
                        color: Color::White,
                    }),
                );
            }
        }
        board
    }

    fn counts_to_hand(counts: [u8; 7], colors: u64) -> Hand {
        const PIECE_TYPES: [PieceType; 7] = [
            PieceType::Pawn,
            PieceType::Lance,
            PieceType::Knight,
            PieceType::Silver,
            PieceType::Gold,
            PieceType::Bishop,
            PieceType::Rook,
        ];
        let mut hand = Hand::default();
        counts
            .iter()
            .zip(PIECE_TYPES.iter())
            .fold(colors, |color_packed_hand, (&count, &pt)| {
                let bits = color_packed_hand & ((1u64 << count) - 1);
                let num_black = bits.count_ones() as u8;
                let num_white = count as u8 - num_black;
                if num_black > 0 {
                    hand.set(
                        Piece {
                            piece_type: pt,
                            color: Color::Black,
                        },
                        num_black,
                    );
                }
                if num_white > 0 {
                    hand.set(
                        Piece {
                            piece_type: pt,
                            color: Color::White,
                        },
                        num_white,
                    );
                }
                color_packed_hand >> count
            });
        hand
    }

    fn decode_bishop_rook(&self) -> (u64, u64) {
        let bishop_or_rook_packed = (self.0[2] >> 54) & 0x3f; // 6 bits
        let bishop_packed = (self.0[2] >> 60) & 0x0f; // 4 bits
        //
        let rook_packed = (!bishop_packed).pdep(bishop_or_rook_packed);
        let bishop_packed = bishop_packed.pdep(bishop_or_rook_packed);

        (bishop_packed, rook_packed)
    }

    fn decode_lance_kinght(&self) -> (u64, u64) {
        let lance_packed = (self.0[3] >> 56) & 0xff; // 8 bits
        let lance_or_knight_packed = (self.0[3] >> 34) & 0x3fffff; // 22 bits
        //
        let knight_packed = (!lance_packed).pdep(lance_or_knight_packed);
        let lance_packed = lance_packed.pdep(lance_or_knight_packed);

        (lance_packed, knight_packed)
    }

    fn decode_silver_gold(&self) -> (u64, u64) {
        let silver_packed = (self.0[1] >> 18) & 0xff; // 8 bits
        let silver_or_gold_packed = (self.0[2] >> 40) & 0x3fff; // 14 bits

        let gold_packed = (!silver_packed).pdep(silver_or_gold_packed);
        let silver_packed = silver_packed.pdep(silver_or_gold_packed);

        (silver_packed, gold_packed)
    }

    fn decode_king_bishop_rook(&self) -> (u64, u64, u64) {
        let (bishop_packed, rook_packed) = self.decode_bishop_rook();
        let silver_or_gold_packed = (self.0[2] >> 40) & 0x3fff; // 14 bits
        let kbr_packed = !silver_or_gold_packed & 0x3fff;
        let bishop_or_rook_packed = (self.0[2] >> 54) & 0x3f; // 6 bits

        let king_packed = (!bishop_or_rook_packed).pdep(kbr_packed);
        let bishop_packed = bishop_packed.pdep(kbr_packed);
        let rook_packed = rook_packed.pdep(kbr_packed);

        (king_packed, bishop_packed, rook_packed)
    }

    fn decode_kbrgs(&self) -> (u64, u64, u64, u64, u64) {
        let (king_packed, bishop_packed, rook_packed) = self.decode_king_bishop_rook();
        let (silver_packed, gold_packed) = self.decode_silver_gold();
        let lance_or_knight_packed = (self.0[3] >> 34) & 0x3fffff; // 22 bits
        let kbrsg_packed = !lance_or_knight_packed & 0x3fffff; // 22 bits
        //
        let king_packed = king_packed.pdep(kbrsg_packed);
        let bishop_packed = bishop_packed.pdep(kbrsg_packed);
        let rook_packed = rook_packed.pdep(kbrsg_packed);
        let silver_packed = silver_packed.pdep(kbrsg_packed);
        let gold_packed = gold_packed.pdep(kbrsg_packed);

        (king_packed, bishop_packed, rook_packed, silver_packed, gold_packed)
    }

    fn decode_non_pawn_pieces(&self) -> [u64; 7] {
        let (king_packed, bishop_packed, rook_packed, silver_packed, gold_packed) = self.decode_kbrgs();
        let (lance_packed, knight_packed) = self.decode_lance_kinght();
        let non_pawn_packed = !self.0[2] & 0x0000_00ff_ffff_ffff; // 40 bits

        [
            king_packed.pdep(non_pawn_packed),
            rook_packed.pdep(non_pawn_packed),
            bishop_packed.pdep(non_pawn_packed),
            gold_packed.pdep(non_pawn_packed),
            silver_packed.pdep(non_pawn_packed),
            knight_packed.pdep(non_pawn_packed),
            lance_packed.pdep(non_pawn_packed),
        ]
    }

    fn decode_pieces(&self) -> ([u64; 14], [u8; 7]) {
        let [
            king_packed,
            rook_packed,
            bishop_packed,
            gold_packed,
            silver_packed,
            knight_packed,
            lance_packed,
        ] = self.decode_non_pawn_pieces();
        let promoted_packed = self.0[3] & 0x0000_0003_ffff_ffff; // 34 bits
        let promoted_packed = promoted_packed.pdep(!king_packed & !gold_packed);

        let pawn_packed = self.0[2] & 0x0000_00ff_ffff_ffff; // 40 bits

        (
            [
                king_packed,
                rook_packed & !promoted_packed,
                bishop_packed & !promoted_packed,
                gold_packed,
                silver_packed & !promoted_packed,
                knight_packed & !promoted_packed,
                lance_packed & !promoted_packed,
                pawn_packed & !promoted_packed,
                rook_packed & promoted_packed,
                bishop_packed & promoted_packed,
                silver_packed & promoted_packed,
                knight_packed & promoted_packed,
                lance_packed & promoted_packed,
                pawn_packed & promoted_packed,
            ],
            [
                18 - pawn_packed.count_ones() as u8,
                4 - lance_packed.count_ones() as u8,
                4 - knight_packed.count_ones() as u8,
                4 - silver_packed.count_ones() as u8,
                4 - gold_packed.count_ones() as u8,
                2 - bishop_packed.count_ones() as u8,
                2 - rook_packed.count_ones() as u8,
            ],
        )
    }

    fn decode_occupied(&self) -> (Bitboard, u32, Color) {
        let occupied_low = self.0[0] & 0x7fff_ffff_ffff_ffff; // 63 bits
        let occupied_high = self.0[1] & 0x0000_0000_0003_ffff; // 18 bits
        let count = occupied_low.count_ones() + occupied_high.count_ones();

        if count > 40 {
            let occupied_low = !occupied_low & 0x7fff_ffff_ffff_ffff; // 63 bits
            let occupied_high = !occupied_high & 0x0000_0000_0003_ffff; // 18 bits
            let count = occupied_low.count_ones() + occupied_high.count_ones();
            let occupied_bb = Bitboard::new(occupied_low, occupied_high);
            (occupied_bb, count, Color::White)
        } else {
            let occupied_bb = Bitboard::new(occupied_low, occupied_high);
            (occupied_bb, count, Color::Black)
        }
    }

    fn decode_color(&self, king_packed: u64, count: u32) -> (u64, u64) {
        let king_black_index_less = (self.0[0] >> 63) & 0x1 == 1;
        let color_packed = (self.0[1] >> 26) & 0x3f_ffff_ffff; // 38 bits
        let king_bit_low = king_packed & king_packed.wrapping_neg();
        let king_bit_high = king_packed ^ king_bit_low;
        let king_color_packed = if king_black_index_less {
            king_bit_low
        } else {
            king_bit_high
        };
        let mask = (1u64 << count) - 1;
        let non_king_packed = !king_packed & mask;
        let color_packed_board = color_packed & ((1u64 << (count - 2)) - 1);
        let color_packed_hand = color_packed >> (count - 2);
        let color_packed_board = color_packed_board.pdep(non_king_packed) | king_color_packed;
        (color_packed_board, color_packed_hand)
    }

    // Unpack the PackedStateInfo into StateInfo.
    pub fn to_state_info(&self, ply: u16) -> StateInfo {
        // Extract occupied bitboard
        let (occupied_bb, count, side_to_move) = self.decode_occupied();
        let (board_pieces, hand_counts) = self.decode_pieces();
        let (color_packed_board, color_packed_hand) = self.decode_color(board_pieces[PieceType::King.index()], count);

        // Reconstruct pieces on board
        let color_bb = Bitboard::new(color_packed_board, 0).pdep(&occupied_bb);
        let type_bb = board_pieces
            .map(|packed| Bitboard::new(packed, 0).pdep(&occupied_bb))
            .try_into()
            .unwrap();
        let board = PackedStateInfo::type_bb_to_piece_grid(&type_bb, &color_bb);
        let hand = PackedStateInfo::counts_to_hand(hand_counts, color_packed_hand);

        StateInfo {
            board: board,
            hand,
            ply,
            side_to_move,
            occupied_bb,
            color_bb: [color_bb.clone(), (&occupied_bb & &!&color_bb)],
            type_bb,
        }
    }

    fn num_pieces(state: &StateInfo, pt: PieceType) -> u32 {
        let on_board = state.type_bb[pt.index()].count() as u32;
        let on_board_promoted = pt
            .promote()
            .map_or(0, |promoted_pt| state.type_bb[promoted_pt.index()].count() as u32);
        let in_hand = state.hand.get(Piece {
            piece_type: pt,
            color: Color::Black,
        }) as u32
            + state.hand.get(Piece {
                piece_type: pt,
                color: Color::White,
            }) as u32;
        on_board + on_board_promoted + in_hand
    }

    // Pack the given StateInfo into PackedStateInfo.
    pub fn from_state_info<const VALIDATE: bool>(state: &StateInfo) -> Option<PackedStateInfo> {
        if VALIDATE {
            if state.occupied_bb.count() > 40 {
                return None;
            }
            let pt_and_counts = [
                (PieceType::Pawn, 18),
                (PieceType::Lance, 4),
                (PieceType::Knight, 4),
                (PieceType::Silver, 4),
                (PieceType::Gold, 4),
                (PieceType::Bishop, 2),
                (PieceType::Rook, 2),
            ];
            for (pt, expected_count) in pt_and_counts.iter() {
                let count = PackedStateInfo::num_pieces(state, *pt);
                if count != *expected_count {
                    return None;
                }
            }
        }

        let mut data = [0u64; 4];
        let mut occupied_low = state.occupied_bb.low();
        let mut occupied_high = state.occupied_bb.high();
        if state.side_to_move == Color::White {
            occupied_low = !occupied_low & 0x7fffffff_ffffffff; // 63bits
            occupied_high = !occupied_high & 0x00000000_0003ffff; // 18bits
        }
        let black_king_index = state.find_king(Color::Black).map(|sq| sq.index() as u8)?;
        let white_king_index = state.find_king(Color::White).map(|sq| sq.index() as u8)?;
        let packed_bb = PackedStateInfo::pack_by_occupied(state);

        // Because occupied_bb.count() <= 40 is guaranteed, it is sufficient to take the low
        // order bits of the result
        let promoted_packed = [
            PieceType::ProPawn,
            PieceType::ProLance,
            PieceType::ProKnight,
            PieceType::ProSilver,
            PieceType::ProBishop,
            PieceType::ProRook,
        ]
        .iter()
        .fold(0u64, |accum, pt| &accum | packed_bb[pt.index()]);
        let pawn_packed = &packed_bb[PieceType::Pawn.index()] | &packed_bb[PieceType::ProPawn.index()];
        let lance_packed = &packed_bb[PieceType::Lance.index()] | &packed_bb[PieceType::ProLance.index()];
        let knight_packed = &packed_bb[PieceType::Knight.index()] | &packed_bb[PieceType::ProKnight.index()];
        let silver_packed = &packed_bb[PieceType::Silver.index()] | &packed_bb[PieceType::ProSilver.index()];
        let gold_packed = &packed_bb[PieceType::Gold.index()];
        let bishop_packed = &packed_bb[PieceType::Bishop.index()] | &packed_bb[PieceType::ProBishop.index()];
        let rook_packed = &packed_bb[PieceType::Rook.index()] | &packed_bb[PieceType::ProRook.index()];
        let king_packed = &packed_bb[PieceType::King.index()];
        if king_packed.count_ones() != 2 {
            return None;
        }
        let lance_or_knight = lance_packed | knight_packed;
        let silver_or_gold = silver_packed | gold_packed;
        let bishop_or_rook = bishop_packed | rook_packed;
        let promoted_packed = promoted_packed.pext(!(king_packed | gold_packed));
        let kbr_packed = king_packed | bishop_or_rook;
        let kbrsg_packed = kbr_packed | silver_or_gold;

        let color_packed_board = state.color_bb[Color::Black.index()]
            .pext_u64(&state.occupied_bb)
            .pext(!king_packed);
        let color_packed_hand = PackedStateInfo::packed_hand_colors(state);
        let color_packed =
            color_packed_board | (color_packed_hand << (state.occupied_bb.count() as u32 - king_packed.count_ones()));

        data[0] = occupied_low;
        if black_king_index < white_king_index {
            data[0] |= 0x8000_0000_0000_0000; // set MSB to 1
        }

        data[1] = occupied_high;
        data[1] |= silver_packed.pext(silver_or_gold) << 18;
        data[1] |= color_packed << 26;

        data[2] = pawn_packed;
        data[2] |= silver_or_gold.pext(kbrsg_packed) << 40;
        data[2] |= bishop_or_rook.pext(kbr_packed) << 54;
        data[2] |= bishop_packed.pext(bishop_or_rook) << 60;

        data[3] = promoted_packed;
        data[3] |= lance_or_knight.pext(!pawn_packed) << 34;
        data[3] |= lance_packed.pext(lance_or_knight) << 56;
        Some(PackedStateInfo(data))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SerializedStateInfo {
    Packed(PackedStateInfo),
    FallbackedSfen(String),
}

impl SerializedStateInfo {
    fn from_state_info(state: &StateInfo) -> SerializedStateInfo {
        if let Some(packed) = PackedStateInfo::from_state_info::<true>(state) {
            SerializedStateInfo::Packed(packed)
        } else {
            SerializedStateInfo::FallbackedSfen(state.generate_sfen().split(" ").take(3).join(" "))
        }
    }

    #[allow(dead_code)]
    fn to_state_info(&self, ply: u16) -> StateInfo {
        match self {
            SerializedStateInfo::Packed(packed) => packed.to_state_info(ply),
            SerializedStateInfo::FallbackedSfen(sfen) => {
                let mut state = StateInfo::default();
                state.set_sfen(sfen).unwrap();
                state.ply = ply;
                state
            }
        }
    }

    fn to_sfen(&self) -> String {
        match self {
            SerializedStateInfo::Packed(packed) => packed.to_state_info(1).generate_sfen().split(" ").take(3).join(" "),
            SerializedStateInfo::FallbackedSfen(sfen) => sfen.clone(),
        }
    }
}


/// Represents a state of the game (board state + history).
///
/// # Examples
///
/// ```
/// use shogi::{Move, Position};
/// use shogi::bitboard::Factory as BBFactory;
/// use shogi::square::consts::*;
///
/// BBFactory::init();
/// let mut pos = Position::new();
/// pos.set_sfen("lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1").unwrap();
///
/// let m = Move::Normal{from: SQ_7G, to: SQ_7F, promote: false};
/// pos.make_move(m).unwrap();
///
/// assert_eq!("lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1 moves 7g7f", pos.to_sfen());
/// ```
#[derive(Debug)]
pub struct Position {
    pub(crate) state: StateInfo,
    move_history: Vec<MoveRecord>,
    position_history: Vec<(SerializedStateInfo, u16)>,
}

/////////////////////////////////////////////////////////////////////////////
// Type implementation
/////////////////////////////////////////////////////////////////////////////

impl Position {
    /// Creates a new instance of `Position` with an empty board.
    pub fn new() -> Position {
        Default::default()
    }

    /////////////////////////////////////////////////////////////////////////
    // Accessors
    /////////////////////////////////////////////////////////////////////////

    /// Returns a piece at the given square.
    pub fn piece_at(&self, sq: Square) -> &Option<Piece> {
        self.state.piece_at(sq)
    }

    /// Returns a bitboard containing pieces of the given player.
    pub fn player_bb(&self, c: Color) -> &Bitboard {
        self.state.player_bb(c)
    }

    /// Returns the number of the given piece in hand.
    pub fn hand(&self, p: Piece) -> u8 {
        self.state.hand(p)
    }

    /// Returns the side to make a move next.
    pub fn side_to_move(&self) -> Color {
        self.state.side_to_move()
    }

    /// Returns the number of plies already completed by the current state.
    pub fn ply(&self) -> u16 {
        self.state.ply()
    }

    /// Returns a history of all moves made since the beginning of the game.
    pub fn move_history(&self) -> &[MoveRecord] {
        &self.move_history
    }

    /// Checks if a player with the given color can declare winning.
    ///
    /// See [the section 25 in 世界コンピュータ将棋選手権 大会ルール][csa] for more detail.
    ///
    /// [csa]: http://www2.computer-shogi.org/wcsc26/rule.pdf#page=9
    pub fn try_declare_winning(&self, c: Color) -> bool {
        self.state.try_declare_winning(c)
    }

    /// Checks if the king with the given color is in check.
    pub fn in_check(&self, c: Color) -> bool {
        self.state.in_check(c)
    }

    /// Returns the position of the king with the given color.
    pub fn find_king(&self, c: Color) -> Option<Square> {
        self.state.find_king(c)
    }

    fn log_position(&mut self) {
        // TODO: SFEN string is used to represent a state of position, but any transformation which uniquely distinguish positions can be used here.
        // Consider light-weight option if generating SFEN string for each move is time-consuming.
        let serialized_position = SerializedStateInfo::from_state_info(&self.state);
        let in_check = self.in_check(self.side_to_move());

        let continuous_check = if in_check {
            let past = if self.position_history.len() >= 2 {
                let record = self.position_history.get(self.position_history.len() - 2).unwrap();
                record.1
            } else {
                0
            };
            past + 1
        } else {
            0
        };

        self.position_history.push((serialized_position, continuous_check));
    }

    /////////////////////////////////////////////////////////////////////////
    // Making a move
    /////////////////////////////////////////////////////////////////////////

    /// Makes the given move. Returns `Err` if the move is invalid or any special condition is met.
    pub fn make_move(&mut self, m: Move) -> Result<(), MoveError> {
        let res = match m {
            Move::Normal { from, to, promote } => self.make_normal_move(from, to, promote)?,
            Move::Drop { to, piece_type } => self.make_drop_move(to, piece_type)?,
        };

        self.move_history.push(res);
        Ok(())
    }

    fn make_normal_move(&mut self, from: Square, to: Square, promoted: bool) -> Result<MoveRecord, MoveError> {
        let record = self.state.make_normal_move(from, to, promoted)?;
        self.log_position();
        self.detect_repetition()?;
        Ok(record)
    }

    fn make_drop_move(&mut self, to: Square, pt: PieceType) -> Result<MoveRecord, MoveError> {
        let record = self.state.make_drop_move(to, pt)?;
        self.log_position();
        self.detect_repetition()?;
        Ok(record)
    }

    /// Returns a list of squares at which a piece of the given color is pinned.
    pub fn pinned_bb(&self, c: Color) -> Bitboard {
        self.state.pinned_bb(c)
    }

    /// Undoes the last move.
    pub fn unmake_move(&mut self) -> Result<(), MoveError> {
        if self.move_history.is_empty() {
            // TODO: error?
            return Ok(());
        }

        let record = self.move_history.pop().unwrap();
        self.state.unmake_move(record)?;
        self.position_history.pop();

        Ok(())
    }

    /// Returns a list of squares to where the given piece at the given square can move.
    pub fn move_candidates(&self, sq: Square, p: Piece) -> Bitboard {
        self.state.move_candidates(sq, p)
    }

    fn detect_repetition(&self) -> Result<(), MoveError> {
        if self.position_history.len() < 9 {
            return Ok(());
        }

        let cur = self.position_history.last().unwrap();

        let mut cnt = 0;
        for (i, entry) in self.position_history.iter().rev().enumerate() {
            if entry.0 == cur.0 {
                cnt += 1;

                if cnt == 4 {
                    let prev = self.position_history.get(self.position_history.len() - 2).unwrap();

                    if cur.1 * 2 >= (i as u16) {
                        return Err(MoveError::PerpetualCheckLose);
                    } else if prev.1 * 2 >= (i as u16) {
                        return Err(MoveError::PerpetualCheckWin);
                    } else {
                        return Err(MoveError::Repetition);
                    }
                }
            }
        }

        Ok(())
    }

    /////////////////////////////////////////////////////////////////////////
    // SFEN serialization / deserialization
    /////////////////////////////////////////////////////////////////////////

    /// Parses the given SFEN string and updates the game state.
    pub fn set_sfen(&mut self, sfen_str: &str) -> Result<(), SfenError> {
        let mut parts = sfen_str.split_whitespace().peekable();

        // Build the initial position from the first 4 fields
        let state_sfen = parts
            .by_ref()
            .take_while(|&s| s != "moves")
            .take(4)
            .collect::<Vec<_>>()
            .join(" ");

        self.state.set_sfen(&state_sfen)?;

        self.position_history.clear();
        self.log_position();

        // Make moves following the initial position, optional.
        if parts.peek() == Some(&"moves") {
            parts.next(); // consume "moves"
            for m in parts {
                if let Some(m) = Move::from_sfen(m) {
                    // Stop if any error occurrs.
                    match self.make_move(m) {
                        Ok(_) => {
                            self.log_position();
                        }
                        Err(_) => break,
                    }
                } else {
                    return Err(SfenError::IllegalMove);
                }
            }
        }

        Ok(())
    }

    /// Converts the current state into SFEN formatted string.
    pub fn to_sfen(&self) -> String {
        if self.position_history.is_empty() {
            return self.state.generate_sfen();
        }

        let initial_sfen = self.position_history.first().unwrap().0.to_sfen();
        if self.move_history.is_empty() {
            return format!("{} {}", initial_sfen, self.ply());
        }

        let mut sfen = format!("{} {} moves", initial_sfen, self.ply() - self.move_history.len() as u16);

        for m in self.move_history.iter() {
            let _ = write!(sfen, " {}", &m.to_sfen());
        }

        sfen
    }
}

/////////////////////////////////////////////////////////////////////////////
// Trait implementations
/////////////////////////////////////////////////////////////////////////////


impl Default for Position {
    fn default() -> Position {
        Position {
            state: Default::default(),
            move_history: Default::default(),
            position_history: Default::default(),
        }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "   9   8   7   6   5   4   3   2   1")?;
        writeln!(f, "+---+---+---+---+---+---+---+---+---+")?;

        for row in 0..9 {
            write!(f, "|")?;
            for file in (0..9).rev() {
                if let Some(ref piece) = *self.piece_at(Square::new(file, row).unwrap()) {
                    write!(f, "{:>3}|", piece.to_string())?;
                } else {
                    write!(f, "   |")?;
                }
            }

            writeln!(f, " {}", (('a' as usize + row as usize) as u8) as char)?;
            writeln!(f, "+---+---+---+---+---+---+---+---+---+")?;
        }

        writeln!(
            f,
            "Side to move: {}",
            if self.state.side_to_move == Color::Black {
                "Black"
            } else {
                "White"
            }
        )?;

        let fmt_hand = |color: Color, f: &mut fmt::Formatter| -> fmt::Result {
            for pt in PieceType::iter().filter(|pt| pt.is_hand_piece()) {
                let pc = Piece { piece_type: pt, color };
                let n = self.state.hand.get(pc);

                if n > 0 {
                    write!(f, "{pc}{n} ")?;
                }
            }
            Ok(())
        };
        write!(f, "Hand (Black): ")?;
        fmt_hand(Color::Black, f)?;
        writeln!(f)?;

        write!(f, "Hand (White): ")?;
        fmt_hand(Color::White, f)?;
        writeln!(f)?;

        write!(f, "Ply: {}", self.state.ply)?;

        Ok(())
    }
}
