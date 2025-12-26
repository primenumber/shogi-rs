use itertools::Itertools;
use std::fmt;
use std::fmt::Write as _;

use crate::bitboard::Factory as BBFactory;
use crate::zobrist::{self, ZobristHash};
use crate::{Bitboard, Color, Hand, Move, MoveError, Piece, PieceType, SfenError, Square};

mod tests;

/// MoveRecord stores information necessary to undo the move.
#[derive(Debug, Clone)]
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
            MoveRecord::Normal {
                from, to, promoted, ..
            } => format!("{}{}{}", from, to, if promoted { "+" } else { "" }),
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

#[derive(Clone)]
struct PieceGrid([Option<Piece>; 81]);

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

/// Represents a state of the game.
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
#[derive(Debug, Clone)]
pub struct Position {
    board: PieceGrid,
    hand: Hand,
    ply: u16,
    side_to_move: Color,
    move_history: Vec<MoveRecord>,
    hash_history: Vec<(ZobristHash, u16)>,
    hash: ZobristHash,
    initial_sfen: Option<String>,
    occupied_bb: Bitboard,
    color_bb: [Bitboard; 2],
    type_bb: [Bitboard; 14],
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
        if c != self.side_to_move {
            return false;
        }

        let king_pos = self.find_king(c);
        if king_pos.is_none() {
            return false;
        }

        let king_pos = king_pos.unwrap();
        if king_pos.relative_rank(c) >= 3 {
            return false;
        }

        let (mut point, count) =
            PieceType::iter()
                .filter(|&pt| pt != PieceType::King)
                .fold((0, 0), |accum, pt| {
                    let unit = match pt {
                        PieceType::Rook
                        | PieceType::Bishop
                        | PieceType::ProRook
                        | PieceType::ProBishop => 5,
                        _ => 1,
                    };

                    let bb = &(&self.type_bb[pt.index()] & &self.color_bb[c.index()])
                        & &BBFactory::promote_zone(c);
                    let count = bb.count() as u8;
                    let point = count * unit;

                    (accum.0 + point, accum.1 + count)
                });

        if count < 10 {
            return false;
        }

        point += PieceType::iter()
            .filter(|pt| pt.is_hand_piece())
            .fold(0, |acc, pt| {
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

    /// Checks if the king with the given color is in check.
    pub fn in_check(&self, c: Color) -> bool {
        if let Some(king_sq) = self.find_king(c) {
            self.is_attacked_by(king_sq, c.flip())
        } else {
            false
        }
    }

    fn in_check_after_move(&self, m: Option<Move>) -> bool {
        let stm = self.side_to_move();
        let Some(king_sq) = self.find_king(stm) else {
            return false;
        };
        let Some(m) = m else {
            return self.is_attacked_by(king_sq, stm.flip());
        };
        match m {
            Move::Normal { to, .. } => {
                let piece = self.piece_at(to).unwrap();
                [
                    PieceType::Lance,
                    PieceType::Rook,
                    PieceType::Bishop,
                    PieceType::ProRook,
                    PieceType::ProBishop,
                    piece.piece_type,
                ]
                .iter()
                .any(|&attack_pt| {
                    self.get_attackers_of_type(attack_pt, king_sq, stm.flip())
                        .is_any()
                })
            }
            Move::Drop { piece_type, .. } => self
                .get_attackers_of_type(piece_type, king_sq, stm.flip())
                .is_any(),
        }
    }

    /// Returns the position of the king with the given color.
    pub fn find_king(&self, c: Color) -> Option<Square> {
        let mut bb = &self.type_bb[PieceType::King.index()] & &self.color_bb[c.index()];
        if bb.is_any() {
            Some(bb.pop())
        } else {
            None
        }
    }

    /// Sets a piece at the given square.
    fn set_piece(&mut self, sq: Square, p: Option<Piece>) {
        self.board.set(sq, p);
    }

    fn is_attacked_by(&self, sq: Square, c: Color) -> bool {
        PieceType::iter().any(|pt| self.get_attackers_of_type(pt, sq, c).is_any())
    }

    fn get_attackers_of_type(&self, pt: PieceType, sq: Square, c: Color) -> Bitboard {
        self.get_attackers_of_type_with_occupied(pt, sq, c, &self.occupied_bb)
    }

    fn log_position(&mut self, m: Option<Move>) {
        let in_check = self.in_check_after_move(m);

        let continuous_check = if in_check {
            let past = if self.hash_history.len() >= 2 {
                let record = self.hash_history.get(self.hash_history.len() - 2).unwrap();
                record.1
            } else {
                0
            };
            past + 1
        } else {
            0
        };

        self.hash_history.push((self.hash, continuous_check));
    }

    /// Computes the Zobrist hash for the current position from scratch.
    fn compute_hash(&self) -> ZobristHash {
        let mut hash: ZobristHash = 0;

        // Hash board pieces
        for sq in Square::iter() {
            if let Some(piece) = *self.piece_at(sq) {
                hash ^= zobrist::board_hash(piece, sq);
            }
        }

        // Hash side to move
        if self.side_to_move == Color::White {
            hash ^= zobrist::side_to_move_hash();
        }

        // Hash hand pieces
        for c in Color::iter() {
            for pt in PieceType::iter().filter(|pt| pt.is_hand_piece()) {
                let piece = Piece {
                    piece_type: pt,
                    color: c,
                };
                let count = self.hand.get(piece);
                hash ^= zobrist::hand_hash(piece, count);
            }
        }

        hash
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

    fn make_normal_move(
        &mut self,
        from: Square,
        to: Square,
        promoted: bool,
    ) -> Result<MoveRecord, MoveError> {
        let stm = self.side_to_move();
        let opponent = stm.flip();

        let moved = self
            .piece_at(from)
            .ok_or(MoveError::Inconsistent("No piece found"))?;

        let captured = *self.piece_at(to);

        if moved.color != stm {
            return Err(MoveError::Inconsistent(
                "The piece is not for the side to move",
            ));
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

        // Update board state
        self.set_piece(from, None);
        self.set_piece(to, Some(placed));
        self.occupied_bb ^= from;
        self.occupied_bb ^= to;
        self.type_bb[moved.piece_type.index()] ^= from;
        self.type_bb[placed.piece_type.index()] ^= to;
        self.color_bb[moved.color.index()] ^= from;
        self.color_bb[placed.color.index()] ^= to;

        // Update hash for moved piece
        self.hash ^= zobrist::board_hash(moved, from);
        self.hash ^= zobrist::board_hash(placed, to);

        if let Some(ref cap) = captured {
            self.occupied_bb ^= to;
            self.type_bb[cap.piece_type.index()] ^= to;
            self.color_bb[cap.color.index()] ^= to;

            // Update hash for captured piece
            self.hash ^= zobrist::board_hash(*cap, to);

            let pc = cap.flip();
            let pc = match pc.unpromote() {
                Some(unpromoted) => unpromoted,
                None => pc,
            };

            // Update hash for hand (remove old count, add new count)
            let old_count = self.hand.get(pc);
            self.hash ^= zobrist::hand_hash(pc, old_count);
            self.hand.increment(pc);
            self.hash ^= zobrist::hand_hash(pc, old_count + 1);
        }

        // Update hash for side to move
        self.hash ^= zobrist::side_to_move_hash();

        if self.in_check(stm) {
            // Undo-ing the move.
            self.set_piece(from, Some(moved));
            self.set_piece(to, captured);
            self.occupied_bb ^= from;
            self.occupied_bb ^= to;
            self.type_bb[moved.piece_type.index()] ^= from;
            self.type_bb[placed.piece_type.index()] ^= to;
            self.color_bb[moved.color.index()] ^= from;
            self.color_bb[placed.color.index()] ^= to;

            // Undo hash for moved piece
            self.hash ^= zobrist::board_hash(moved, from);
            self.hash ^= zobrist::board_hash(placed, to);

            if let Some(ref cap) = captured {
                self.occupied_bb ^= to;
                self.type_bb[cap.piece_type.index()] ^= to;
                self.color_bb[cap.color.index()] ^= to;

                // Undo hash for captured piece
                self.hash ^= zobrist::board_hash(*cap, to);

                let pc = cap.flip();
                let pc = match pc.unpromote() {
                    Some(unpromoted) => unpromoted,
                    None => pc,
                };

                // Undo hash for hand
                let new_count = self.hand.get(pc);
                self.hash ^= zobrist::hand_hash(pc, new_count);
                self.hand.decrement(pc);
                self.hash ^= zobrist::hand_hash(pc, new_count - 1);
            }

            // Undo hash for side to move
            self.hash ^= zobrist::side_to_move_hash();

            return Err(MoveError::InCheck);
        }

        self.side_to_move = opponent;
        self.ply += 1;

        self.log_position(Some(Move::Normal {
            from,
            to,
            promote: promoted,
        }));
        self.detect_repetition()?;

        Ok(MoveRecord::Normal {
            from,
            to,
            placed,
            captured,
            promoted,
        })
    }

    fn make_drop_move(&mut self, to: Square, pt: PieceType) -> Result<MoveRecord, MoveError> {
        let stm = self.side_to_move();
        let opponent = stm.flip();

        if self.piece_at(to).is_some() {
            return Err(MoveError::Inconsistent("There is already a piece in `to`"));
        }

        let pc = Piece {
            piece_type: pt,
            color: stm,
        };

        if self.hand(pc) == 0 {
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

        // Update board state
        self.set_piece(to, Some(pc));
        self.occupied_bb ^= to;
        self.type_bb[pc.piece_type.index()] ^= to;
        self.color_bb[pc.color.index()] ^= to;

        // Update hash for dropped piece
        self.hash ^= zobrist::board_hash(pc, to);

        // Update hash for hand (remove old count, add new count)
        let old_count = self.hand.get(pc);
        self.hash ^= zobrist::hand_hash(pc, old_count);
        self.hash ^= zobrist::hand_hash(pc, old_count - 1);

        // Update hash for side to move
        self.hash ^= zobrist::side_to_move_hash();

        if self.in_check(stm) {
            // Undo-ing the move.
            self.set_piece(to, None);
            self.occupied_bb ^= to;
            self.type_bb[pc.piece_type.index()] ^= to;
            self.color_bb[pc.color.index()] ^= to;

            // Undo hash for dropped piece
            self.hash ^= zobrist::board_hash(pc, to);

            // Undo hash for hand
            self.hash ^= zobrist::hand_hash(pc, old_count - 1);
            self.hash ^= zobrist::hand_hash(pc, old_count);

            // Undo hash for side to move
            self.hash ^= zobrist::side_to_move_hash();

            return Err(MoveError::InCheck);
        }

        self.hand.decrement(pc);
        self.side_to_move = opponent;
        self.ply += 1;

        self.log_position(Some(Move::Drop { to, piece_type: pt }));
        self.detect_repetition()?;

        Ok(MoveRecord::Drop { to, piece: pc })
    }

    /// Returns a list of squares at which a piece of the given color is pinned.
    pub fn pinned_bb(&self, c: Color) -> Bitboard {
        let ksq = self.find_king(c);
        if ksq.is_none() {
            return Bitboard::empty();
        }
        let ksq = ksq.unwrap();

        [
            (
                PieceType::Rook,
                BBFactory::rook_attack(ksq, &Bitboard::empty()),
            ),
            (
                PieceType::ProRook,
                BBFactory::rook_attack(ksq, &Bitboard::empty()),
            ),
            (
                PieceType::Bishop,
                BBFactory::bishop_attack(ksq, &Bitboard::empty()),
            ),
            (
                PieceType::ProBishop,
                BBFactory::bishop_attack(ksq, &Bitboard::empty()),
            ),
            (
                PieceType::Lance,
                BBFactory::lance_attack(c, ksq, &Bitboard::empty()),
            ),
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

    /// Undoes the last move.
    pub fn unmake_move(&mut self) -> Result<(), MoveError> {
        if self.move_history.is_empty() {
            // TODO: error?
            return Ok(());
        }

        let last = self.move_history.pop().unwrap();
        match last {
            MoveRecord::Normal {
                from,
                to,
                ref placed,
                ref captured,
                promoted,
            } => {
                if self.piece_at(from).is_some() {
                    return Err(MoveError::Inconsistent(
                        "`from` of the move is filled by another piece",
                    ));
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
                    return Err(MoveError::Inconsistent(
                        "Expected piece is not found in `to`",
                    ));
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
                    return Err(MoveError::Inconsistent(
                        "Expected piece is not found in `to`",
                    ));
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
        self.hash_history.pop();

        // Restore hash from history
        if let Some((prev_hash, _)) = self.hash_history.last() {
            self.hash = *prev_hash;
        } else {
            // Recompute hash if history is empty
            self.hash = self.compute_hash();
        }

        Ok(())
    }

    /// Returns a list of squares to where the given piece at the given square can move.
    pub fn move_candidates(&self, sq: Square, p: Piece) -> Bitboard {
        self.move_candidates_with_occupied(sq, p, &self.occupied_bb)
    }

    fn all_normal_move_candidates(&self, from: Square, p: Piece) -> Vec<Move> {
        let mut moves = Vec::new();
        let targets = self.move_candidates(from, p);

        for to in targets {
            let promoteable = from.in_promotion_zone(p.color) || to.in_promotion_zone(p.color);
            if p.piece_type.promote().is_some() && promoteable {
                if p.is_placeable_at(to) {
                    moves.push(Move::Normal {
                        from,
                        to,
                        promote: false,
                    });
                }
                moves.push(Move::Normal {
                    from,
                    to,
                    promote: true,
                });
            } else {
                moves.push(Move::Normal {
                    from,
                    to,
                    promote: false,
                });
            }
        }

        moves
    }

    fn all_drop_move_candidates(&self, c: Color) -> Vec<Move> {
        let mut moves = Vec::new();

        for pt in PieceType::iter().filter(|pt| pt.is_hand_piece()) {
            let pc = Piece {
                piece_type: pt,
                color: c,
            };
            let n = self.hand.get(pc);
            if n == 0 {
                continue;
            }

            for to in Square::iter() {
                if pc.is_placeable_at(to) && self.piece_at(to).is_none() {
                    moves.push(Move::Drop { to, piece_type: pt });
                }
            }
        }

        moves
    }

    pub fn all_move_candidates(&self, c: Color) -> Vec<Move> {
        let mut moves = Vec::new();

        for sq in self.color_bb[c.index()] {
            let pc = self.piece_at(sq).unwrap();
            moves.extend(self.all_normal_move_candidates(sq, pc));
        }

        moves.extend(self.all_drop_move_candidates(c));

        moves
    }

    /// Checks if the given move is legal without modifying the board.
    ///
    /// This function validates the move and checks that it doesn't leave the king in check.
    /// Unlike `make_move`, this function does not update the board state.
    pub fn is_legal(&self, m: Move) -> bool {
        match m {
            Move::Normal { from, to, promote } => self.is_legal_normal_move(from, to, promote),
            Move::Drop { to, piece_type } => self.is_legal_drop_move(to, piece_type),
        }
    }

    fn is_legal_normal_move(&self, from: Square, to: Square, promote: bool) -> bool {
        let stm = self.side_to_move();

        // Check if there is a piece at `from`
        let Some(moved) = *self.piece_at(from) else {
            return false;
        };

        // Check if the piece belongs to the side to move
        if moved.color != stm {
            return false;
        }

        // Check promotion conditions
        if promote && !from.in_promotion_zone(stm) && !to.in_promotion_zone(stm) {
            return false;
        }

        // Check if the piece can move to `to`
        if !self.move_candidates(from, moved).any(|sq| sq == to) {
            return false;
        }

        // Check if the piece can be placed at `to` without promotion
        if !promote && !moved.is_placeable_at(to) {
            return false;
        }

        // Check if the piece type can promote
        if promote && moved.promote().is_none() {
            return false;
        }

        // Check if the move leaves the king in check
        !self.leaves_king_in_check_normal(from, to, moved)
    }

    fn is_legal_drop_move(&self, to: Square, pt: PieceType) -> bool {
        let stm = self.side_to_move();
        let opponent = stm.flip();

        // Check if `to` is empty
        if self.piece_at(to).is_some() {
            return false;
        }

        let pc = Piece {
            piece_type: pt,
            color: stm,
        };

        // Check if the piece is in hand
        if self.hand(pc) == 0 {
            return false;
        }

        // Check if the piece can be placed at `to`
        if !pc.is_placeable_at(to) {
            return false;
        }

        // Pawn-specific rules
        if pt == PieceType::Pawn {
            // Nifu check
            for i in 0..9 {
                if let Some(fp) = *self.piece_at(Square::new(to.file(), i).unwrap()) {
                    if fp == pc {
                        return false;
                    }
                }
            }

            // Uchifuzume check
            if let Some(king_sq) = to.shift(0, if stm == Color::Black { -1 } else { 1 }) {
                if let Some(
                    king_pc @ Piece {
                        piece_type: PieceType::King,
                        ..
                    },
                ) = *self.piece_at(king_sq)
                {
                    if king_pc.color == opponent {
                        // Check if any opponent's piece can capture the dropped pawn
                        let pinned = self.pinned_bb(opponent);

                        let not_attacked = PieceType::iter()
                            .filter(|&pt| pt != PieceType::King)
                            .flat_map(|pt| self.get_attackers_of_type(pt, to, opponent))
                            .all(|sq| (&pinned & sq).is_any());

                        if not_attacked {
                            // Temporarily add the pawn to check if king can escape
                            let virtual_occupied = &self.occupied_bb | to;

                            let is_attacked = |sq: Square| {
                                if let Some(pc) = *self.piece_at(sq) {
                                    if pc.color == opponent {
                                        return true;
                                    }
                                }
                                self.is_attacked_by_with_occupied(sq, stm, &virtual_occupied)
                            };

                            let uchifuzume =
                                self.move_candidates(king_sq, king_pc).all(is_attacked);

                            if uchifuzume {
                                return false;
                            }
                        }
                    }
                }
            }
        }

        // Check if the move leaves the king in check
        !self.leaves_king_in_check_drop(to)
    }

    /// Checks if the king is attacked by a specific color with a custom occupied bitboard.
    fn is_attacked_by_with_occupied(&self, sq: Square, c: Color, occupied: &Bitboard) -> bool {
        PieceType::iter().any(|pt| {
            self.get_attackers_of_type_with_occupied(pt, sq, c, occupied)
                .is_any()
        })
    }

    /// Gets attackers of a specific type with a custom occupied bitboard.
    fn get_attackers_of_type_with_occupied(
        &self,
        pt: PieceType,
        sq: Square,
        c: Color,
        occupied: &Bitboard,
    ) -> Bitboard {
        let bb = &self.type_bb[pt.index()] & &self.color_bb[c.index()];

        if bb.is_empty() {
            return bb;
        }

        let attack_pc = Piece {
            piece_type: pt,
            color: c,
        };

        &bb & &self.move_candidates_with_occupied(sq, attack_pc.flip(), occupied)
    }

    /// Returns move candidates with a custom occupied bitboard.
    fn move_candidates_with_occupied(&self, sq: Square, p: Piece, occupied: &Bitboard) -> Bitboard {
        let bb = match p.piece_type {
            PieceType::Rook => BBFactory::rook_attack(sq, occupied),
            PieceType::Bishop => BBFactory::bishop_attack(sq, occupied),
            PieceType::Lance => BBFactory::lance_attack(p.color, sq, occupied),
            PieceType::ProRook => {
                &BBFactory::rook_attack(sq, occupied)
                    | &BBFactory::attacks_from(PieceType::King, p.color, sq)
            }
            PieceType::ProBishop => {
                &BBFactory::bishop_attack(sq, occupied)
                    | &BBFactory::attacks_from(PieceType::King, p.color, sq)
            }
            PieceType::ProSilver
            | PieceType::ProKnight
            | PieceType::ProLance
            | PieceType::ProPawn => BBFactory::attacks_from(PieceType::Gold, p.color, sq),
            pt => BBFactory::attacks_from(pt, p.color, sq),
        };

        &bb & &!&self.color_bb[p.color.index()]
    }

    /// Checks if a normal move leaves the king in check.
    fn leaves_king_in_check_normal(&self, from: Square, to: Square, moved: Piece) -> bool {
        let stm = self.side_to_move();
        let opponent = stm.flip();

        // Determine king's position after the move
        let king_sq = if moved.piece_type == PieceType::King {
            to
        } else {
            match self.find_king(stm) {
                Some(sq) => sq,
                None => return false,
            }
        };

        // Build virtual occupied bitboard
        // After the move: `from` is empty, `to` is occupied by the moved piece
        // If there was a piece at `to`, it is captured and removed
        // So virtual_occupied should be: (occupied - from) | to
        let mut virtual_occupied = &self.occupied_bb ^ from; // Remove `from`
        virtual_occupied |= to; // Add `to`

        // Check if opponent can attack king_sq with the virtual occupied board
        self.is_king_attacked_with_virtual_board(king_sq, opponent, &virtual_occupied, Some(to))
    }

    /// Checks if a drop move leaves the king in check.
    fn leaves_king_in_check_drop(&self, to: Square) -> bool {
        let stm = self.side_to_move();
        let opponent = stm.flip();

        let king_sq = match self.find_king(stm) {
            Some(sq) => sq,
            None => return false,
        };

        // Virtual occupied: current occupied + to
        let virtual_occupied = &self.occupied_bb | to;

        // Check if opponent can attack king_sq with the virtual occupied board
        self.is_king_attacked_with_virtual_board(king_sq, opponent, &virtual_occupied, None)
    }

    /// Checks if the king at king_sq is attacked by color c with the given occupied board.
    /// captured_sq is a square where the opponent's piece has been captured (if any).
    fn is_king_attacked_with_virtual_board(
        &self,
        king_sq: Square,
        attacker: Color,
        virtual_occupied: &Bitboard,
        captured_sq: Option<Square>,
    ) -> bool {
        for pt in PieceType::iter() {
            let mut attackers = &self.type_bb[pt.index()] & &self.color_bb[attacker.index()];

            // Exclude captured piece if any
            if let Some(cap_sq) = captured_sq {
                let cap_sq_bb = &Bitboard::empty() | cap_sq;
                attackers = &attackers & &!&cap_sq_bb;
            }

            if attackers.is_empty() {
                continue;
            }

            // Compute attack pattern from king_sq for the opposite piece
            // (reverse attack to find attackers)
            let attack_pc = Piece {
                piece_type: pt,
                color: attacker,
            };
            let attack_bb =
                self.move_candidates_with_occupied(king_sq, attack_pc.flip(), virtual_occupied);

            if (&attackers & &attack_bb).is_any() {
                return true;
            }
        }

        false
    }

    fn detect_repetition(&self) -> Result<(), MoveError> {
        if self.hash_history.len() < 9 {
            return Ok(());
        }

        let cur = self.hash_history.last().unwrap();

        let mut cnt = 0;
        for (i, entry) in self.hash_history.iter().rev().enumerate() {
            if entry.0 == cur.0 {
                cnt += 1;

                if cnt == 4 {
                    let prev = self.hash_history.get(self.hash_history.len() - 2).unwrap();

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
        let mut parts = sfen_str.split_whitespace();

        // Build the initial position, all parts are required.
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

        self.hash = self.compute_hash();
        self.hash_history.clear();
        self.initial_sfen = Some(self.generate_sfen());
        self.log_position(None);

        // Make moves following the initial position, optional.
        if let Some("moves") = parts.next() {
            for m in parts {
                if let Some(m) = Move::from_sfen(m) {
                    // Stop if any error occurrs.
                    match self.make_move(m) {
                        Ok(_) => {}
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
        match &self.initial_sfen {
            None => self.generate_sfen(),
            Some(initial) => {
                if self.move_history.is_empty() {
                    initial.clone()
                } else {
                    let mut sfen = format!("{} moves", initial);
                    for m in self.move_history.iter() {
                        let _ = write!(sfen, " {}", &m.to_sfen());
                    }
                    sfen
                }
            }
        }
    }

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

    fn parse_sfen_stm(&mut self, s: &str) -> Result<(), SfenError> {
        self.side_to_move = match s {
            "b" => Color::Black,
            "w" => Color::White,
            _ => return Err(SfenError::IllegalSideToMove),
        };
        Ok(())
    }

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
                        Some(p) => self
                            .hand
                            .set(p, if num_pieces == 0 { 1 } else { num_pieces }),
                        None => return Err(SfenError::IllegalPieceType),
                    };
                    num_pieces = 0;
                }
            }
        }

        Ok(())
    }

    fn parse_sfen_ply(&mut self, s: &str) -> Result<(), SfenError> {
        self.ply = s.parse()?;
        Ok(())
    }

    fn generate_sfen(&self) -> String {
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

        let color = if self.side_to_move == Color::Black {
            "b"
        } else {
            "w"
        };

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
}

/////////////////////////////////////////////////////////////////////////////
// Trait implementations
/////////////////////////////////////////////////////////////////////////////

impl Default for Position {
    fn default() -> Position {
        Position {
            side_to_move: Color::Black,
            board: PieceGrid([None; 81]),
            hand: Default::default(),
            ply: 1,
            move_history: Default::default(),
            hash_history: Default::default(),
            hash: 0,
            initial_sfen: None,
            occupied_bb: Default::default(),
            color_bb: Default::default(),
            type_bb: Default::default(),
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
            if self.side_to_move == Color::Black {
                "Black"
            } else {
                "White"
            }
        )?;

        let fmt_hand = |color: Color, f: &mut fmt::Formatter| -> fmt::Result {
            for pt in PieceType::iter().filter(|pt| pt.is_hand_piece()) {
                let pc = Piece {
                    piece_type: pt,
                    color,
                };
                let n = self.hand.get(pc);

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

        write!(f, "Ply: {}", self.ply)?;

        Ok(())
    }
}
