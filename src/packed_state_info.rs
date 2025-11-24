use bitintr::{Pdep, Pext};
use itertools::Itertools;

use crate::state_info::{PieceGrid, StateInfo};
use crate::{Bitboard, Color, Hand, Piece, PieceType};

/// A compact representation of StateInfo using 256 bits.
///
/// # Layout
///
/// | Description        | Size (bits) | Offset (bits) |
/// |--------------------|-------------|---------------|
/// | occupied_low       | 63          | 0             |
/// | king_order         | 1           | 63            |
/// | occupied_high      | 18          | 64            |
/// | is_silver          | 8           | 81            |
/// | is_black           | 38          | 89            |
/// | is_pawn            | 40          | 128           |
/// | is_silver_or_gold  | 14          | 168           |
/// | is_bishop_or_rook  | 6           | 182           |
/// | is_bishop          | 4           | 188           |
/// | is_promoted        | 34          | 192           |
/// | is_lance_or_knight | 22          | 226           |
/// | is_lance           | 8           | 248           |
/// | **Total**          | **256**     |               |
///
/// ## Explanation of fields
///
/// ### occupied_low, occupied_high
///
/// bitboard of occupied squares. If side to move is White, the bits are inverted.
/// Since there are at most 40 pieces on the board, the number of set bits is at most 40 when side to move is Black.
/// If side to move is White, the number of set bits is at least 41.
///
/// ### king_order
///
/// Indicates which king has the smaller index on the board.
/// 1: Black king has smaller index, 0: White king has smaller index.
///
/// ### piece type bits
///
/// The pieces on the board are encoded using a prefix-free Huffman code as follows:
/// Promoted pieces are treated as unpromoted pieces here.
///
/// | Piece Type | Huffman Code       |
/// |------------|--------------------|
/// | Pawn       | 1                  |
/// | Lance      | 011                |
/// | Knight     | 010                |
/// | Silver     | 0011               |
/// | Gold       | 0010               |
/// | Bishop     | 000111             |
/// | Rook       | 000110             |
/// | King       | 00010              |
///
/// The bits indicating piece types are stored in multiple fields as follows:
///
/// | Field              | nth bit of Huffman code | Include Piece Types              |
/// |--------------------|-------------------------|----------------------------------|
/// | is_pawn            | 0                       | All types                        |
/// | is_lance_or_knight | 1                       | Except Pawn                      |
/// | is_lance           | 2                       | Lance, Knight                    |
/// | is_silver_or_gold  | 2                       | Silver, Gold, Bishop, Rook, King |
/// | is_silver          | 3                       | Silver, Gold                     |
/// | is_bishop_or_rook  | 3                       | Bishop, Rook, King               |
/// | is_bishop          | 4                       | Bishop, Rook                     |
///
/// If there are fewer than 40 pieces on the board, fill the upper bits with 0.
///
/// ### is_promoted
///
/// Indicates whether each piece on the board is promoted or not.
/// Only pieces that can be promoted are included here.
/// If there are fewer than 34 promotable pieces on the board, fill the upper bits with 0.
///
/// ### is_black
///
/// Indicates the colors of pieces on the board except for kings and hand pieces.
/// The colors are packed into bits corresponding to the pieces on the board
/// and the pieces in hand.
/// Lower bits correspond to pieces on the board, and higher bits correspond to pieces in hand.
///
/// ### hand pieces
///
/// The colors of hand pieces are packed into bits.
/// Each piece type uses (num_black + num_white) bits, where
/// num_black bits are set to 1, and num_white bits are set to 0.
/// Piece types are packed in the order of
/// Pawn, Lance, Knight, Silver, Gold, Bishop, Rook (from LSB to MSB).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedStateInfo([u64; 4]);

impl PackedStateInfo {
    fn type_bb_to_piece_grid(bbs: &[Bitboard; 14], color_bb: &Bitboard) -> PieceGrid {
        let mut board = PieceGrid([None; 81]);
        for (pt, bb) in PieceType::iter().zip(bbs.iter()) {
            let mut black_bb = *bb & *color_bb;
            let mut white_bb = *bb & !*color_bb;
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
            color_bb: [color_bb.clone(), (occupied_bb & !color_bb)],
            type_bb,
        }
    }

    fn encode_occupied_and_turn(state: &StateInfo) -> (u64, u64) {
        let mut occupied_low = state.occupied_bb.low();
        let mut occupied_high = state.occupied_bb.high();
        if state.side_to_move == Color::White {
            occupied_low = !occupied_low & 0x7fffffff_ffffffff; // 63bits
            occupied_high = !occupied_high & 0x00000000_0003ffff; // 18bits
        }
        (occupied_low, occupied_high)
    }

    fn encode_promoted(state: &StateInfo) -> u64 {
        let promoted_bb = state.type_bb[PieceType::ProPawn.index()]
            | state.type_bb[PieceType::ProLance.index()]
            | state.type_bb[PieceType::ProKnight.index()]
            | state.type_bb[PieceType::ProSilver.index()]
            | state.type_bb[PieceType::ProBishop.index()]
            | state.type_bb[PieceType::ProRook.index()];
        promoted_bb.pext_u64(&state.occupied_bb)
    }

    fn pack_board_pieces(state: &StateInfo) -> [u64; 8] {
        [
            state.type_bb[PieceType::King.index()].pext_u64(&state.occupied_bb),
            (state.type_bb[PieceType::Rook.index()] | state.type_bb[PieceType::ProRook.index()])
                .pext_u64(&state.occupied_bb),
            (state.type_bb[PieceType::Bishop.index()] | state.type_bb[PieceType::ProBishop.index()])
                .pext_u64(&state.occupied_bb),
            state.type_bb[PieceType::Gold.index()].pext_u64(&state.occupied_bb),
            (state.type_bb[PieceType::Silver.index()] | state.type_bb[PieceType::ProSilver.index()])
                .pext_u64(&state.occupied_bb),
            (state.type_bb[PieceType::Knight.index()] | state.type_bb[PieceType::ProKnight.index()])
                .pext_u64(&state.occupied_bb),
            (state.type_bb[PieceType::Lance.index()] | state.type_bb[PieceType::ProLance.index()])
                .pext_u64(&state.occupied_bb),
            (state.type_bb[PieceType::Pawn.index()] | state.type_bb[PieceType::ProPawn.index()])
                .pext_u64(&state.occupied_bb),
        ]
    }

    fn encode_board_pieces(state: &StateInfo, data: &mut [u64; 4]) -> [u64; 8] {
        let pieces_packed = PackedStateInfo::pack_board_pieces(state);
        let [
            king_packed,
            rook_packed,
            bishop_packed,
            gold_packed,
            silver_packed,
            knight_packed,
            lance_packed,
            pawn_packed,
        ] = pieces_packed;

        let lance_or_knight = lance_packed | knight_packed;
        let silver_or_gold = silver_packed | gold_packed;
        let bishop_or_rook = bishop_packed | rook_packed;
        let kbr_packed = king_packed | bishop_or_rook;
        let kbrsg_packed = kbr_packed | silver_or_gold;

        data[1] ^= silver_packed.pext(silver_or_gold) << 18;

        data[2] ^= pawn_packed;
        data[2] ^= silver_or_gold.pext(kbrsg_packed) << 40;
        data[2] ^= bishop_or_rook.pext(kbr_packed) << 54;
        data[2] ^= bishop_packed.pext(bishop_or_rook) << 60;

        data[3] ^= lance_or_knight.pext(!pawn_packed) << 34;
        data[3] ^= lance_packed.pext(lance_or_knight) << 56;

        pieces_packed
    }

    // Pack hand pieces' colors into bits.
    // Each piece type uses (num_black + num_white) bits, where
    // num_black bits are set to 1, and num_white bits are set to 0.
    // piece types are packed in the order of
    // Pawn, Lance, Knight, Silver, Gold, Bishop, Rook (from LSB to MSB).
    fn encode_hand_colors(state: &StateInfo, packed_pieces: [u64; 8]) -> Option<u64> {
        let mut accum = 0u64;
        // the order is reversed
        let pts = [
            (PieceType::Rook, 2),
            (PieceType::Bishop, 2),
            (PieceType::Gold, 4),
            (PieceType::Silver, 4),
            (PieceType::Knight, 4),
            (PieceType::Lance, 4),
            (PieceType::Pawn, 18),
        ];
        for (pt, num_pieces) in pts.iter() {
            let num_black = state.hand.get(Piece {
                piece_type: *pt,
                color: Color::Black,
            });
            let num_white = state.hand.get(Piece {
                piece_type: *pt,
                color: Color::White,
            });
            if num_black + num_white + packed_pieces[pt.index()].count_ones() as u8 != *num_pieces {
                return None;
            }
            accum <<= num_black + num_white;
            accum |= (1u64 << num_black) - 1;
        }
        Some(accum)
    }

    // Pack the given StateInfo into PackedStateInfo.
    pub fn from_state_info(state: &StateInfo) -> Option<PackedStateInfo> {
        if state.occupied_bb.count() > 40 {
            return None;
        }

        let mut data = [0u64; 4];
        let (occupied_low, occupied_high) = PackedStateInfo::encode_occupied_and_turn(state);
        let black_king_index = state.find_king(Color::Black).map(|sq| sq.index() as u8)?;
        let white_king_index = state.find_king(Color::White).map(|sq| sq.index() as u8)?;

        // Because occupied_bb.count() <= 40 is guaranteed, it is sufficient to take the low
        // order bits of the result
        let promoted_packed = PackedStateInfo::encode_promoted(state);
        let pieces_packed = PackedStateInfo::encode_board_pieces(state, &mut data);
        let king_packed = pieces_packed[0];
        let gold_packed = pieces_packed[3];
        if king_packed.count_ones() != 2 {
            return None;
        }
        let promoted_packed = promoted_packed.pext(!(king_packed | gold_packed));

        let color_packed_board = state.color_bb[Color::Black.index()]
            .pext_u64(&state.occupied_bb)
            .pext(!king_packed);
        let color_packed_hand = PackedStateInfo::encode_hand_colors(state, pieces_packed)?;
        let color_packed =
            color_packed_board | (color_packed_hand << (state.occupied_bb.count() as u32 - king_packed.count_ones()));
        let king_order = if black_king_index < white_king_index { 1 } else { 0 };

        data[0] ^= occupied_low;
        data[0] ^= king_order << 63;
        data[1] ^= occupied_high;
        data[1] ^= color_packed << 26;
        data[3] ^= promoted_packed;
        Some(PackedStateInfo(data))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SerializedStateInfo {
    Packed(PackedStateInfo),
    FallbackedSfen(String),
}

impl SerializedStateInfo {
    pub(crate) fn from_state_info(state: &StateInfo) -> SerializedStateInfo {
        if let Some(packed) = PackedStateInfo::from_state_info(state) {
            SerializedStateInfo::Packed(packed)
        } else {
            SerializedStateInfo::FallbackedSfen(state.generate_sfen().split(" ").take(3).join(" "))
        }
    }

    #[allow(dead_code)]
    pub(crate) fn to_state_info(&self, ply: u16) -> StateInfo {
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

    pub(crate) fn to_sfen(&self) -> String {
        match self {
            SerializedStateInfo::Packed(packed) => packed.to_state_info(1).generate_sfen().split(" ").take(3).join(" "),
            SerializedStateInfo::FallbackedSfen(sfen) => sfen.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitboard::Factory as BBFactory;
    use crate::{Color, Piece, Position, Square};

    fn setup() {
        BBFactory::init();
    }

    #[test]
    fn packed_position_roundtrip() {
        setup();

        let test_cases = [
            // Standard starting position
            "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            // Position with pieces in hand and promoted pieces (from set_sfen_custom test)
            "lnsgk+Lpnl/1p5+B1/p1+Pps1ppp/9/9/9/P+r1PPpPPP/1R7/LNSGKGSN1 w BGP2p 1024",
            // Position from make_normal_move test
            "l6nl/5+P1gk/2np1S3/p1p4Pp/3P2Sp1/1PPb2P1P/P5GS1/R8/LN4bKL w GR5pnsg 1",
        ];

        for (i, &sfen) in test_cases.iter().enumerate() {
            let mut original_pos = Position::new();
            original_pos
                .set_sfen(sfen)
                .unwrap_or_else(|_| panic!("failed to parse SFEN at case #{i}"));

            // Pack the state
            let serialized = SerializedStateInfo::from_state_info(&original_pos.state);

            // Unpack the state
            let restored_state = serialized.to_state_info(1);

            // Verify board pieces match
            for sq in Square::iter() {
                assert_eq!(
                    original_pos.piece_at(sq),
                    restored_state.piece_at(sq),
                    "piece mismatch at {sq} in case #{i} (SFEN: {sfen})"
                );
            }

            // Verify hand pieces match
            for pt in PieceType::iter().filter(|pt| pt.is_hand_piece()) {
                for c in [Color::Black, Color::White] {
                    let piece = Piece {
                        piece_type: pt,
                        color: c,
                    };
                    assert_eq!(
                        original_pos.hand(piece),
                        restored_state.hand(piece),
                        "hand mismatch for {piece:?} in case #{i} (SFEN: {sfen})"
                    );
                }
            }

            // Verify side to move matches
            assert_eq!(
                original_pos.side_to_move(),
                restored_state.side_to_move(),
                "side_to_move mismatch in case #{i} (SFEN: {sfen})"
            );

            // Verify bitboards match
            assert_eq!(
                original_pos.state.occupied_bb, restored_state.occupied_bb,
                "occupied_bb mismatch in case #{i} (SFEN: {sfen})"
            );
            for c in [Color::Black, Color::White] {
                assert_eq!(
                    original_pos.player_bb(c),
                    restored_state.player_bb(c),
                    "color_bb[{c:?}] mismatch in case #{i} (SFEN: {sfen})"
                );
            }
        }
    }
}
