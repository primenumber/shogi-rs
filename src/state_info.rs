use itertools::Itertools;
use std::fmt;

use crate::bitboard::Factory as BBFactory;
use crate::position::MoveRecord;
use crate::{Bitboard, Color, Hand, MoveError, Piece, PieceType, SfenError, Square};

pub struct PieceGrid(pub(crate) [Option<Piece>; 81]);

impl PieceGrid {
    pub fn get(&self, sq: Square) -> &Option<Piece> {
        &self.0[sq.index()]
    }

    pub fn set(&mut self, sq: Square, pc: Option<Piece>) {
        self.0[sq.index()] = pc;
    }
}

impl fmt::Debug for PieceGrid {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "PieceGrid {{ ")?;

        for pc in self.0.iter() {
            write!(fmt, "{pc:?} ")?;
        }
        write!(fmt, "}}")
    }
}

/// Represents the current board state (without history).
#[derive(Debug)]
pub struct StateInfo {
    pub(crate) board: PieceGrid,
    pub(crate) hand: Hand,
    pub(crate) ply: u16,
    pub(crate) side_to_move: Color,
    pub(crate) occupied_bb: Bitboard,
    pub(crate) color_bb: [Bitboard; 2],
    pub(crate) type_bb: [Bitboard; 14],
}

impl StateInfo {
    /// Returns a piece at the given square.
    pub fn piece_at(&self, sq: Square) -> &Option<Piece> {
        self.board.get(sq)
    }

    /// Returns a bitboard containing pieces of the given player.
    pub fn player_bb(&self, c: Color) -> &Bitboard {
        &self.color_bb[c.index()]
    }

    /// Returns the number of the given piece in hand.
    pub fn hand(&self, p: Piece) -> u8 {
        self.hand.get(p)
    }

    /// Returns the side to make a move next.
    pub fn side_to_move(&self) -> Color {
        self.side_to_move
    }

    /// Returns the number of plies already completed by the current state.
    pub fn ply(&self) -> u16 {
        self.ply
    }

    /// Returns the position of the king with the given color.
    pub fn find_king(&self, c: Color) -> Option<Square> {
        let mut bb = &self.type_bb[PieceType::King.index()] & &self.color_bb[c.index()];
        if bb.is_any() { Some(bb.pop()) } else { None }
    }

    /// Sets a piece at the given square.
    pub(crate) fn set_piece(&mut self, sq: Square, p: Option<Piece>) {
        self.board.set(sq, p);
    }

    /// Returns true if the given color is in check.
    pub fn in_check(&self, c: Color) -> bool {
        if let Some(king_sq) = self.find_king(c) {
            self.is_attacked_by(king_sq, c.flip())
        } else {
            false
        }
    }

    /// Returns true if the given square is attacked by the given color.
    pub(crate) fn is_attacked_by(&self, sq: Square, c: Color) -> bool {
        PieceType::iter()
            .flat_map(|pt| self.get_attackers_of_type(pt, sq, c))
            .any(|_| true)
    }

    /// Returns a bitboard containing pieces of the given type that attack the given square.
    pub(crate) fn get_attackers_of_type(&self, pt: PieceType, sq: Square, c: Color) -> Bitboard {
        let bb = &self.type_bb[pt.index()] & &self.color_bb[c.index()];

        if bb.is_empty() {
            return bb;
        }

        let attack_pc = Piece {
            piece_type: pt,
            color: c,
        };

        &bb & &self.move_candidates(sq, attack_pc.flip())
    }

    /// Returns a bitboard containing squares to where the given piece at the given square can move.
    pub fn move_candidates(&self, sq: Square, p: Piece) -> Bitboard {
        let bb = match p.piece_type {
            PieceType::Rook => BBFactory::rook_attack(sq, &self.occupied_bb),
            PieceType::Bishop => BBFactory::bishop_attack(sq, &self.occupied_bb),
            PieceType::Lance => BBFactory::lance_attack(p.color, sq, &self.occupied_bb),
            PieceType::ProRook => {
                &BBFactory::rook_attack(sq, &self.occupied_bb) | &BBFactory::attacks_from(PieceType::King, p.color, sq)
            }
            PieceType::ProBishop => {
                &BBFactory::bishop_attack(sq, &self.occupied_bb)
                    | &BBFactory::attacks_from(PieceType::King, p.color, sq)
            }
            PieceType::ProSilver | PieceType::ProKnight | PieceType::ProLance | PieceType::ProPawn => {
                BBFactory::attacks_from(PieceType::Gold, p.color, sq)
            }
            pt => BBFactory::attacks_from(pt, p.color, sq),
        };

        &bb & &!&self.color_bb[p.color.index()]
    }

    /// Returns a bitboard containing squares where pieces are pinned.
    pub fn pinned_bb(&self, c: Color) -> Bitboard {
        let ksq = self.find_king(c);
        if ksq.is_none() {
            return Bitboard::empty();
        }
        let ksq = ksq.unwrap();

        [
            (PieceType::Rook, BBFactory::rook_attack(ksq, &Bitboard::empty())),
            (PieceType::ProRook, BBFactory::rook_attack(ksq, &Bitboard::empty())),
            (PieceType::Bishop, BBFactory::bishop_attack(ksq, &Bitboard::empty())),
            (PieceType::ProBishop, BBFactory::bishop_attack(ksq, &Bitboard::empty())),
            (PieceType::Lance, BBFactory::lance_attack(c, ksq, &Bitboard::empty())),
        ]
        .iter()
        .fold(Bitboard::empty(), |mut accum, &(pt, ref mask)| {
            let bb = &(&self.type_bb[pt.index()] & &self.color_bb[c.flip().index()]) & mask;

            for psq in bb {
                let between = &BBFactory::between(ksq, psq) & &self.occupied_bb;
                if between.count() == 1 && (&between & &self.color_bb[c.index()]).is_any() {
                    accum |= &between;
                }
            }

            accum
        })
    }

    /// Returns true if the given color can declare winning.
    pub fn try_declare_winning(&self, c: Color) -> bool {
        if self.side_to_move != c {
            return false;
        }

        if let Some(king_sq) = self.find_king(c) {
            if !king_sq.in_promotion_zone(c) {
                return false;
            }
        } else {
            return false;
        }

        let (mut point, count) = PieceType::iter()
            .filter(|&pt| pt != PieceType::King)
            .fold((0, 0), |accum, pt| {
                let unit = match pt {
                    PieceType::Rook | PieceType::Bishop | PieceType::ProRook | PieceType::ProBishop => 5,
                    _ => 1,
                };

                let bb = &(&self.type_bb[pt.index()] & &self.color_bb[c.index()]) & &BBFactory::promote_zone(c);
                let count = bb.count() as u8;
                let point = count * unit;

                (accum.0 + point, accum.1 + count)
            });

        if count < 10 {
            return false;
        }

        point += PieceType::iter().filter(|pt| pt.is_hand_piece()).fold(0, |acc, pt| {
            let num = self.hand.get(Piece {
                piece_type: pt,
                color: c,
            });
            let pp = match pt {
                PieceType::Rook | PieceType::Bishop => 5,
                _ => 1,
            };

            acc + num * pp
        });

        let lowerbound = match c {
            Color::Black => 28,
            Color::White => 27,
        };
        if point < lowerbound {
            return false;
        }

        if self.in_check(c) {
            return false;
        }

        true
    }

    /// Generates SFEN string representing the current board state.
    pub fn generate_sfen(&self) -> String {
        let board = (0..9)
            .map(|row| {
                let mut s = String::new();
                let mut num_spaces = 0;
                for file in (0..9).rev() {
                    match *self.piece_at(Square::new(file, row).unwrap()) {
                        Some(pc) => {
                            if num_spaces > 0 {
                                s.push_str(&num_spaces.to_string());
                                num_spaces = 0;
                            }

                            s.push_str(&pc.to_string());
                        }
                        None => num_spaces += 1,
                    }
                }

                if num_spaces > 0 {
                    s.push_str(&num_spaces.to_string());
                }

                s
            })
            .join("/");

        let color = if self.side_to_move == Color::Black { "b" } else { "w" };

        let mut hand = [Color::Black, Color::White]
            .iter()
            .map(|c| {
                PieceType::iter()
                    .filter(|pt| pt.is_hand_piece())
                    .map(|pt| {
                        let pc = Piece {
                            piece_type: pt,
                            color: *c,
                        };
                        let n = self.hand.get(pc);

                        if n == 0 {
                            "".to_string()
                        } else if n == 1 {
                            format!("{pc}")
                        } else {
                            format!("{n}{pc}")
                        }
                    })
                    .join("")
            })
            .join("");

        if hand.is_empty() {
            hand = "-".to_string();
        }

        format!("{} {} {} {}", board, color, hand, self.ply)
    }

    /// Parses SFEN board representation and updates the board state.
    fn parse_sfen_board(&mut self, s: &str) -> Result<(), SfenError> {
        let rows = s.split('/');

        self.occupied_bb = Bitboard::empty();
        self.color_bb = Default::default();
        self.type_bb = Default::default();

        for (i, row) in rows.enumerate() {
            if i >= 9 {
                return Err(SfenError::IllegalBoardState);
            }

            let mut j = 0;

            let mut is_promoted = false;
            for c in row.chars() {
                match c {
                    '+' => {
                        is_promoted = true;
                    }
                    n if n.is_ascii_digit() => {
                        if let Some(n) = n.to_digit(10) {
                            for _ in 0..n {
                                if j >= 9 {
                                    return Err(SfenError::IllegalBoardState);
                                }

                                let sq = Square::new(8 - j, i as u8).unwrap();
                                self.set_piece(sq, None);

                                j += 1;
                            }
                        }
                    }
                    s => match Piece::from_sfen(s) {
                        Some(mut piece) => {
                            if j >= 9 {
                                return Err(SfenError::IllegalBoardState);
                            }

                            if is_promoted {
                                if let Some(promoted) = piece.piece_type.promote() {
                                    piece.piece_type = promoted;
                                } else {
                                    return Err(SfenError::IllegalPieceType);
                                }
                            }

                            let sq = Square::new(8 - j, i as u8).unwrap();
                            self.set_piece(sq, Some(piece));
                            self.occupied_bb |= sq;
                            self.color_bb[piece.color.index()] |= sq;
                            self.type_bb[piece.piece_type.index()] |= sq;
                            j += 1;

                            is_promoted = false;
                        }
                        None => return Err(SfenError::IllegalPieceType),
                    },
                }
            }
        }

        Ok(())
    }

    /// Parses SFEN side-to-move field and updates the state.
    fn parse_sfen_stm(&mut self, s: &str) -> Result<(), SfenError> {
        self.side_to_move = match s {
            "b" => Color::Black,
            "w" => Color::White,
            _ => return Err(SfenError::IllegalSideToMove),
        };
        Ok(())
    }

    /// Parses SFEN hand pieces field and updates the hand.
    fn parse_sfen_hand(&mut self, s: &str) -> Result<(), SfenError> {
        if s == "-" {
            self.hand.clear();
            return Ok(());
        }

        let mut num_pieces: u8 = 0;
        for c in s.chars() {
            match c {
                n if n.is_ascii_digit() => {
                    if let Some(n) = n.to_digit(10) {
                        num_pieces = num_pieces * 10 + (n as u8);
                    }
                }
                s => {
                    match Piece::from_sfen(s) {
                        Some(p) => self.hand.set(p, if num_pieces == 0 { 1 } else { num_pieces }),
                        None => return Err(SfenError::IllegalPieceType),
                    };
                    num_pieces = 0;
                }
            }
        }

        Ok(())
    }

    /// Parses SFEN ply field and updates the ply count.
    fn parse_sfen_ply(&mut self, s: &str) -> Result<(), SfenError> {
        self.ply = s.parse()?;
        Ok(())
    }

    /// Sets the state from SFEN string (without move history).
    /// This parses only the first 4 fields: board, side-to-move, hand, ply.
    pub fn set_sfen(&mut self, sfen_str: &str) -> Result<(), SfenError> {
        let mut parts = sfen_str.split_whitespace();

        parts
            .next()
            .ok_or(SfenError::MissingDataFields)
            .and_then(|s| self.parse_sfen_board(s))?;
        parts
            .next()
            .ok_or(SfenError::MissingDataFields)
            .and_then(|s| self.parse_sfen_stm(s))?;
        parts
            .next()
            .ok_or(SfenError::MissingDataFields)
            .and_then(|s| self.parse_sfen_hand(s))?;
        parts
            .next()
            .ok_or(SfenError::MissingDataFields)
            .and_then(|s| self.parse_sfen_ply(s))?;

        Ok(())
    }

    /// Makes a normal move (not a drop). Updates the board state and validates the move.
    /// Does NOT update position history or check for repetition.
    pub(crate) fn make_normal_move(
        &mut self,
        from: Square,
        to: Square,
        promoted: bool,
    ) -> Result<MoveRecord, MoveError> {
        let stm = self.side_to_move;
        let opponent = stm.flip();

        let moved = self.piece_at(from).ok_or(MoveError::Inconsistent("No piece found"))?;

        let captured = *self.piece_at(to);

        if moved.color != stm {
            return Err(MoveError::Inconsistent("The piece is not for the side to move"));
        }

        if promoted && !from.in_promotion_zone(stm) && !to.in_promotion_zone(stm) {
            return Err(MoveError::Inconsistent("The piece cannot promote"));
        }

        if !self.move_candidates(from, moved).any(|sq| sq == to) {
            return Err(MoveError::Inconsistent("The piece cannot move to there"));
        }

        if !promoted && !moved.is_placeable_at(to) {
            return Err(MoveError::NonMovablePiece);
        }

        let placed = if promoted {
            match moved.promote() {
                Some(promoted) => promoted,
                None => return Err(MoveError::Inconsistent("This type of piece cannot promote")),
            }
        } else {
            moved
        };

        self.set_piece(from, None);
        self.set_piece(to, Some(placed));
        self.occupied_bb ^= from;
        self.occupied_bb ^= to;
        self.type_bb[moved.piece_type.index()] ^= from;
        self.type_bb[placed.piece_type.index()] ^= to;
        self.color_bb[moved.color.index()] ^= from;
        self.color_bb[placed.color.index()] ^= to;

        if let Some(ref cap) = captured {
            self.occupied_bb ^= to;
            self.type_bb[cap.piece_type.index()] ^= to;
            self.color_bb[cap.color.index()] ^= to;
            let pc = cap.flip();
            let pc = match pc.unpromote() {
                Some(unpromoted) => unpromoted,
                None => pc,
            };
            self.hand.increment(pc);
        }

        if self.in_check(stm) {
            // Undo the move.
            self.set_piece(from, Some(moved));
            self.set_piece(to, captured);
            self.occupied_bb ^= from;
            self.occupied_bb ^= to;
            self.type_bb[moved.piece_type.index()] ^= from;
            self.type_bb[placed.piece_type.index()] ^= to;
            self.color_bb[moved.color.index()] ^= from;
            self.color_bb[placed.color.index()] ^= to;

            if let Some(ref cap) = captured {
                self.occupied_bb ^= to;
                self.type_bb[cap.piece_type.index()] ^= to;
                self.color_bb[cap.color.index()] ^= to;
                let pc = cap.flip();
                let pc = match pc.unpromote() {
                    Some(unpromoted) => unpromoted,
                    None => pc,
                };
                self.hand.decrement(pc);
            }

            return Err(MoveError::InCheck);
        }

        self.side_to_move = opponent;
        self.ply += 1;

        Ok(MoveRecord::Normal {
            from,
            to,
            placed,
            captured,
            promoted,
        })
    }

    /// Makes a drop move. Updates the board state and validates the move.
    /// Does NOT update position history or check for repetition.
    pub(crate) fn make_drop_move(&mut self, to: Square, pt: PieceType) -> Result<MoveRecord, MoveError> {
        let stm = self.side_to_move;
        let opponent = stm.flip();

        if self.piece_at(to).is_some() {
            return Err(MoveError::Inconsistent("There is already a piece in `to`"));
        }

        let pc = Piece {
            piece_type: pt,
            color: stm,
        };

        if self.hand.get(pc) == 0 {
            return Err(MoveError::Inconsistent("The piece is not in the hand"));
        }

        if !pc.is_placeable_at(to) {
            return Err(MoveError::NonMovablePiece);
        }

        if pc.piece_type == PieceType::Pawn {
            // Nifu check.
            for i in 0..9 {
                if let Some(fp) = *self.piece_at(Square::new(to.file(), i).unwrap()) {
                    if fp == pc {
                        return Err(MoveError::Nifu);
                    }
                }
            }

            // Uchifuzume check.
            if let Some(king_sq) = to.shift(0, if stm == Color::Black { -1 } else { 1 }) {
                // Is the dropped pawn attacking the opponent's king?
                if let Some(
                    pc @ Piece {
                        piece_type: PieceType::King,
                        ..
                    },
                ) = *self.piece_at(king_sq)
                {
                    if pc.color == opponent {
                        // can any opponent's piece attack the dropped pawn?
                        let pinned = self.pinned_bb(opponent);

                        let not_attacked = PieceType::iter()
                            .filter(|&pt| pt != PieceType::King)
                            .flat_map(|pt| self.get_attackers_of_type(pt, to, opponent))
                            .all(|sq| (&pinned & sq).is_any());

                        if not_attacked {
                            // the dropped pawn may block bishop's moves
                            self.occupied_bb ^= to;
                            // can the opponent's king evade?
                            let is_attacked = |sq| {
                                if let Some(pc) = *self.piece_at(sq) {
                                    if pc.color == opponent {
                                        return true;
                                    }
                                }

                                self.is_attacked_by(sq, stm)
                            };
                            let uchifuzume = self.move_candidates(king_sq, pc).all(is_attacked);
                            self.occupied_bb ^= to;

                            if uchifuzume {
                                return Err(MoveError::Uchifuzume);
                            }
                        }
                    }
                }
            }
        }

        self.set_piece(to, Some(pc));
        self.occupied_bb ^= to;
        self.type_bb[pc.piece_type.index()] ^= to;
        self.color_bb[pc.color.index()] ^= to;

        if self.in_check(stm) {
            // Undo the move.
            self.set_piece(to, None);
            self.occupied_bb ^= to;
            self.type_bb[pc.piece_type.index()] ^= to;
            self.color_bb[pc.color.index()] ^= to;
            return Err(MoveError::InCheck);
        }

        self.hand.decrement(pc);
        self.side_to_move = opponent;
        self.ply += 1;

        Ok(MoveRecord::Drop { to, piece: pc })
    }

    /// Undoes a move given its MoveRecord. Updates the board state.
    /// Does NOT update position history.
    pub(crate) fn unmake_move(&mut self, record: MoveRecord) -> Result<(), MoveError> {
        match record {
            MoveRecord::Normal {
                from,
                to,
                ref placed,
                ref captured,
                promoted,
            } => {
                if self.piece_at(from).is_some() {
                    return Err(MoveError::Inconsistent("`from` of the move is filled by another piece"));
                }

                let moved = if promoted {
                    match placed.unpromote() {
                        Some(unpromoted) => unpromoted,
                        None => return Err(MoveError::Inconsistent("Cannot unpromoted the piece")),
                    }
                } else {
                    *placed
                };
                if *self.piece_at(to) != Some(*placed) {
                    return Err(MoveError::Inconsistent("Expected piece is not found in `to`"));
                }

                self.set_piece(from, Some(moved));
                self.set_piece(to, *captured);
                self.occupied_bb ^= from;
                self.occupied_bb ^= to;
                self.type_bb[moved.piece_type.index()] ^= from;
                self.type_bb[placed.piece_type.index()] ^= to;
                self.color_bb[moved.color.index()] ^= from;
                self.color_bb[placed.color.index()] ^= to;

                if let Some(ref cap) = *captured {
                    self.occupied_bb ^= to;
                    self.type_bb[cap.piece_type.index()] ^= to;
                    self.color_bb[cap.color.index()] ^= to;
                    let unpromoted_cap = cap.unpromote().unwrap_or(*cap);
                    self.hand.decrement(unpromoted_cap.flip());
                }
            }
            MoveRecord::Drop { to, piece } => {
                if *self.piece_at(to) != Some(piece) {
                    return Err(MoveError::Inconsistent("Expected piece is not found in `to`"));
                }

                self.set_piece(to, None);
                self.occupied_bb ^= to;
                self.type_bb[piece.piece_type.index()] ^= to;
                self.color_bb[piece.color.index()] ^= to;
                self.hand.increment(piece);
            }
        };

        self.side_to_move = self.side_to_move.flip();
        self.ply -= 1;

        Ok(())
    }
}

impl Default for StateInfo {
    fn default() -> StateInfo {
        StateInfo {
            side_to_move: Color::Black,
            board: PieceGrid([None; 81]),
            hand: Default::default(),
            ply: 1,
            occupied_bb: Default::default(),
            color_bb: Default::default(),
            type_bb: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::square::consts::*;

    fn setup() {
        BBFactory::init();
    }

    #[test]
    fn in_check() {
        setup();

        let test_cases = [
            (
                "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
                false,
                false,
            ),
            ("9/3r5/9/9/6B2/9/9/9/3K5 b P 1", true, false),
            (
                "ln2r1knl/2gb1+Rg2/4Pp1p1/p1pp1sp1p/1N2pN1P1/2P2PP2/PP1G1S2R/1SG6/LK6L w 2PSp 1",
                false,
                true,
            ),
            (
                "lnsg1gsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSG1GSNL b - 1",
                false,
                false,
            ),
        ];

        let mut state = StateInfo::default();
        for case in test_cases.iter() {
            state.set_sfen(case.0).expect("failed to parse SFEN string");
            assert_eq!(case.1, state.in_check(Color::Black));
            assert_eq!(case.2, state.in_check(Color::White));
        }
    }

    #[test]
    fn find_king() {
        setup();

        let test_cases = [
            (
                "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
                Some(SQ_5I),
                Some(SQ_5A),
            ),
            ("9/3r5/9/9/6B2/9/9/9/3K5 b P 1", Some(SQ_6I), None),
            (
                "ln2r1knl/2gb1+Rg2/4Pp1p1/p1pp1sp1p/1N2pN1P1/2P2PP2/PP1G1S2R/1SG6/LK6L w 2PSp 1",
                Some(SQ_8I),
                Some(SQ_3A),
            ),
            (
                "lnsg1gsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSG1GSNL b - 1",
                None,
                None,
            ),
        ];

        let mut state = StateInfo::default();
        for case in test_cases.iter() {
            state.set_sfen(case.0).expect("failed to parse SFEN string");
            assert_eq!(case.1, state.find_king(Color::Black));
            assert_eq!(case.2, state.find_king(Color::White));
        }
    }

    #[test]
    fn player_bb() {
        setup();

        let cases: &[(&str, &[Square], &[Square])] = &[
            (
                "R6gk/9/8p/9/4p4/9/9/8L/B8 b - 1",
                &[SQ_9A, SQ_1H, SQ_9I],
                &[SQ_2A, SQ_1A, SQ_1C, SQ_5E],
            ),
            ("9/3r5/9/9/6B2/9/9/9/3K5 b P 1", &[SQ_3E, SQ_6I], &[SQ_6B]),
        ];

        let mut state = StateInfo::default();
        for case in cases {
            state.set_sfen(case.0).expect("failed to parse SFEN string");
            let black = state.player_bb(Color::Black);
            let white = state.player_bb(Color::White);

            assert_eq!(case.1.len(), black.count() as usize);
            for sq in case.1 {
                assert!((black & *sq).is_any());
            }

            assert_eq!(case.2.len(), white.count() as usize);
            for sq in case.2 {
                assert!((white & *sq).is_any());
            }
        }
    }

    #[test]
    fn pinned_bb() {
        setup();

        let cases: &[(&str, &[Square], &[Square])] =
            &[("R6gk/9/8p/9/4p4/9/9/8L/B8 b - 1", &[], &[SQ_2A, SQ_1C, SQ_5E])];

        let mut state = StateInfo::default();
        for case in cases {
            state.set_sfen(case.0).expect("failed to parse SFEN string");
            let black = state.pinned_bb(Color::Black);
            let white = state.pinned_bb(Color::White);

            assert_eq!(case.1.len(), black.count());
            for sq in case.1 {
                assert!((&black & *sq).is_any());
            }

            assert_eq!(case.2.len(), white.count());
            for sq in case.2 {
                assert!((&white & *sq).is_any());
            }
        }
    }

    #[test]
    fn move_candidates() {
        setup();

        let mut state = StateInfo::default();
        state
            .set_sfen("lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1")
            .expect("failed to parse SFEN string");

        let mut sum = 0;
        for sq in Square::iter() {
            let pc = state.piece_at(sq);

            if let Some(pc) = *pc {
                if pc.color == state.side_to_move() {
                    sum += state.move_candidates(sq, pc).count();
                }
            }
        }

        assert_eq!(30, sum);
    }

    #[test]
    fn try_declare_winning() {
        setup();

        let mut state = StateInfo::default();

        state
            .set_sfen("lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1")
            .expect("failed to parse SFEN string");
        assert!(!state.try_declare_winning(Color::Black));
        assert!(!state.try_declare_winning(Color::White));

        state
            .set_sfen("1K7/+NG+N+NGG3/P+S+P+P+PS3/9/7s1/9/+b+rppp+p+s1+p/3+p1+bk2/9 b R4L7Pgnp 1")
            .expect("failed to parse SFEN string");
        assert!(state.try_declare_winning(Color::Black));
        assert!(!state.try_declare_winning(Color::White));

        state
            .set_sfen(
                "1K6l/1+N7/+PG2+Ns1p1/2+N5p/6p2/3+b4P/4+p+p+bs1/+r1s4+lk/1g1g3+r1 w \
                 Gns2l11p 1",
            )
            .expect("failed to parse SFEN string");
        assert!(!state.try_declare_winning(Color::Black));
        assert!(state.try_declare_winning(Color::White));

        state
            .set_sfen(
                "1K6l/1+N7/+PG2+Ns1p1/2+N5p/6p2/3+b4P/4+p+p+bs1/+r1s4+lk/1g1g3+r1 b \
                 Gns2l11p 1",
            )
            .expect("failed to parse SFEN string");
        assert!(!state.try_declare_winning(Color::Black));
        assert!(!state.try_declare_winning(Color::White));

        state
            .set_sfen(
                "1K6l/1+N7/+PG2+Ns1p1/2+N5p/6p2/3+b4P/4+p+p+bs1/+r1s4+l1/1g1g3+r1 b \
                 Gns2l11p 1",
            )
            .expect("failed to parse SFEN string");
        assert!(!state.try_declare_winning(Color::Black));
        assert!(!state.try_declare_winning(Color::White));

        state
            .set_sfen(
                "1K6l/1+N7/+PG2+Ns1p1/2+N5p/6p2/1k1+b4P/4+p+p+bs1/+r1s4+l1/1g1g3+r1 b \
                 Gns2l11p 1",
            )
            .expect("failed to parse SFEN string");
        assert!(!state.try_declare_winning(Color::Black));
        assert!(!state.try_declare_winning(Color::White));

        state
            .set_sfen(
                "1K6l/1+N7/+PG2+Ns1p1/2+N5p/6p2/3+b4P/4+p+p+bs1/+r1s4+lk/1g1g3+rG w \
                 ns2l11p 1",
            )
            .expect("failed to parse SFEN string");
        assert!(!state.try_declare_winning(Color::Black));
        assert!(!state.try_declare_winning(Color::White));

        state
            .set_sfen("1K6l/1+N7/+PG2+Ns1p1/2+N5p/6p2/3+b4P/5+p+bs1/+r1s4+lk/1g1g3+rG w ns2l12p 1")
            .expect("failed to parse SFEN string");
        assert!(!state.try_declare_winning(Color::Black));
        assert!(!state.try_declare_winning(Color::White));
    }
}
