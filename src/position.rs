use std::fmt;
use std::fmt::Write as _;

use crate::state_info::StateInfo;
use crate::zobrist::ZobristHash;
use crate::{Bitboard, Color, Move, MoveError, Piece, PieceType, SfenError, Square};

mod test;

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
    /// Position history using Zobrist hash for fast repetition detection.
    /// Stores (hash, continuous_check_count) tuples.
    position_history: Vec<(u64, u16)>,
    zobrist: ZobristHash,
    current_hash: u64,
    /// Initial SFEN for reconstructing full game notation
    initial_sfen: Option<String>,
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

    /// Returns the current state information.
    pub fn state_info(&self) -> &StateInfo {
        &self.state
    }

    /// Returns the current Zobrist hash value.
    pub fn current_hash(&self) -> u64 {
        self.current_hash
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

    fn in_check_after_move(&self, move_record: Option<MoveRecord>) -> bool {
        self.state.in_check_after_move(move_record)
    }

    /// Returns the position of the king with the given color.
    pub fn find_king(&self, c: Color) -> Option<Square> {
        self.state.find_king(c)
    }

    /// Computes the Zobrist hash for the current position from scratch
    fn compute_hash(&self) -> u64 {
        let mut hash = 0u64;

        // Hash board pieces
        for sq in Square::iter() {
            if let Some(piece) = self.piece_at(sq) {
                hash ^= self.zobrist.piece_hash(*piece, sq);
            }
        }

        // Hash hand pieces
        for &color in &[Color::Black, Color::White] {
            for &pt in &[
                PieceType::Pawn,
                PieceType::Lance,
                PieceType::Knight,
                PieceType::Silver,
                PieceType::Gold,
                PieceType::Bishop,
                PieceType::Rook,
            ] {
                let count = self.hand(Piece { piece_type: pt, color });
                if count > 0 {
                    hash ^= self.zobrist.hand_hash(Piece { piece_type: pt, color }, count);
                }
            }
        }

        // Hash side to move
        hash ^= self.zobrist.side_hash(self.side_to_move());

        hash
    }

    fn log_position(&mut self, move_record: Option<MoveRecord>) {
        let in_check = self.in_check_after_move(move_record);

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

        self.position_history.push((self.current_hash, continuous_check));
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
        // Get piece information before move
        let moved = match *self.piece_at(from) {
            Some(piece) => piece,
            None => return Err(MoveError::Inconsistent("No piece found")),
        };
        let captured = *self.piece_at(to);

        // Make the move
        let record = self.state.make_normal_move(from, to, promoted)?;

        // Update hash incrementally
        // Remove moved piece from 'from' square
        self.current_hash ^= self.zobrist.piece_hash(moved, from);

        // Add placed piece to 'to' square
        let placed = if promoted {
            moved.promote().ok_or(MoveError::Inconsistent("Cannot promote"))?
        } else {
            moved
        };
        self.current_hash ^= self.zobrist.piece_hash(placed, to);

        // Remove captured piece from 'to' square (if any)
        if let Some(cap) = captured {
            self.current_hash ^= self.zobrist.piece_hash(cap, to);
            // Add unpromoted captured piece to hand
            let unpromoted = cap.unpromote().unwrap_or(cap);
            let flipped = unpromoted.flip();
            // Remove old hand count
            let old_count = self.hand(flipped).saturating_sub(1);
            if old_count > 0 {
                self.current_hash ^= self.zobrist.hand_hash(flipped, old_count);
            }
            // Add new hand count
            let new_count = self.hand(flipped);
            self.current_hash ^= self.zobrist.hand_hash(flipped, new_count);
        }

        // Toggle side to move
        self.current_hash ^= self.zobrist.toggle_side();

        self.log_position(Some(record.clone()));
        self.detect_repetition()?;
        Ok(record)
    }

    fn make_drop_move(&mut self, to: Square, pt: PieceType) -> Result<MoveRecord, MoveError> {
        let stm = self.side_to_move();
        let piece = Piece {
            piece_type: pt,
            color: stm,
        };

        // Get hand count before drop
        let old_count = self.hand(piece);

        // Make the move
        let record = self.state.make_drop_move(to, pt)?;

        // Update hash incrementally
        // Add dropped piece to board
        self.current_hash ^= self.zobrist.piece_hash(piece, to);

        // Remove from hand (old count)
        if old_count > 0 {
            self.current_hash ^= self.zobrist.hand_hash(piece, old_count);
        }
        // Add new hand count
        let new_count = self.hand(piece);
        if new_count > 0 {
            self.current_hash ^= self.zobrist.hand_hash(piece, new_count);
        }

        // Toggle side to move
        self.current_hash ^= self.zobrist.toggle_side();

        self.log_position(Some(record.clone()));
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

        // Restore hash from history
        if let Some(&(hash, _)) = self.position_history.last() {
            self.current_hash = hash;
        } else {
            // No history, recompute from scratch
            self.current_hash = self.compute_hash();
        }

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
            // Compare hash values
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
        let state_sfen_parts: Vec<&str> = parts.by_ref().take_while(|&s| s != "moves").take(4).collect();
        let state_sfen = state_sfen_parts.join(" ");

        self.state.set_sfen(&state_sfen)?;

        // Store initial SFEN (first 3 fields: board, side, hand)
        self.initial_sfen = Some(state_sfen_parts.iter().take(3).copied().collect::<Vec<_>>().join(" "));

        // Compute initial hash
        self.current_hash = self.compute_hash();

        self.position_history.clear();
        self.move_history.clear();
        self.log_position(None);

        // Make moves following the initial position, optional.
        if parts.peek() == Some(&"moves") {
            parts.next(); // consume "moves"
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
        if self.move_history.is_empty() {
            return self.state.generate_sfen();
        }

        // Use stored initial SFEN if available
        let initial_ply = self.ply().saturating_sub(self.move_history.len() as u16);
        let initial_sfen = self
            .initial_sfen
            .as_ref()
            .map(|s| format!("{} {}", s, initial_ply))
            .unwrap_or_else(|| self.state.generate_sfen());

        let mut sfen = format!("{} moves", initial_sfen);

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
        let zobrist = ZobristHash::new();
        let state = StateInfo::default();
        // Compute initial hash for empty position
        let mut hash = 0u64;
        hash ^= zobrist.side_hash(state.side_to_move());

        Position {
            state,
            move_history: Default::default(),
            position_history: Default::default(),
            zobrist,
            current_hash: hash,
            initial_sfen: None,
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
