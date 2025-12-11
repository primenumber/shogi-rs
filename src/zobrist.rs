//! Zobrist hashing for shogi positions
//!
//! Zobrist hashing provides a fast incremental hash function for position comparison.
//! It is primarily used for efficient repetition detection.

use crate::{Color, Piece, PieceType, Square};

/// Zobrist hash generator for shogi positions
///
/// Generates 64-bit hash values that can be incrementally updated as moves are made/unmade.
/// Hash values are deterministic (using a fixed seed) for reproducibility.
#[derive(Clone, Debug)]
pub struct ZobristHash {
    // piece_square[piece_type][color][square]
    piece_square: [[[u64; 81]; 2]; 14],
    // hand_pieces[piece_type][color][count]
    hand_pieces: [[[u64; 20]; 2]; 7],
    // side to move
    side_to_move: u64,
}

impl ZobristHash {
    /// Creates a new ZobristHash instance with deterministic random values
    pub fn new() -> Self {
        Self::with_seed(0x517cc1b727220a95)
    }

    /// Creates a ZobristHash with a specific seed (for testing)
    pub fn with_seed(seed: u64) -> Self {
        // Simple xorshift64 PRNG for deterministic random number generation
        let mut rng = seed;
        let mut next_random = || -> u64 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };

        let mut piece_square = [[[0u64; 81]; 2]; 14];
        for pt_idx in 0..14 {
            for color_idx in 0..2 {
                for sq_idx in 0..81 {
                    piece_square[pt_idx][color_idx][sq_idx] = next_random();
                }
            }
        }

        let mut hand_pieces = [[[0u64; 20]; 2]; 7];
        for pt_idx in 0..7 {
            for color_idx in 0..2 {
                for count in 0..20 {
                    hand_pieces[pt_idx][color_idx][count] = next_random();
                }
            }
        }

        let side_to_move = next_random();

        Self {
            piece_square,
            hand_pieces,
            side_to_move,
        }
    }

    fn piece_type_index(pt: PieceType) -> usize {
        match pt {
            PieceType::Pawn => 0,
            PieceType::Lance => 1,
            PieceType::Knight => 2,
            PieceType::Silver => 3,
            PieceType::Gold => 4,
            PieceType::Bishop => 5,
            PieceType::Rook => 6,
            PieceType::King => 7,
            PieceType::ProPawn => 8,
            PieceType::ProLance => 9,
            PieceType::ProKnight => 10,
            PieceType::ProSilver => 11,
            PieceType::ProBishop => 12,
            PieceType::ProRook => 13,
        }
    }

    fn hand_piece_index(pt: PieceType) -> Option<usize> {
        match pt {
            PieceType::Pawn => Some(0),
            PieceType::Lance => Some(1),
            PieceType::Knight => Some(2),
            PieceType::Silver => Some(3),
            PieceType::Gold => Some(4),
            PieceType::Bishop => Some(5),
            PieceType::Rook => Some(6),
            _ => None,
        }
    }

    fn color_index(color: Color) -> usize {
        match color {
            Color::Black => 0,
            Color::White => 1,
        }
    }

    /// Returns the hash value for a piece on a square
    pub fn piece_hash(&self, piece: Piece, sq: Square) -> u64 {
        let pt_idx = Self::piece_type_index(piece.piece_type);
        let color_idx = Self::color_index(piece.color);
        let sq_idx = sq.index();
        self.piece_square[pt_idx][color_idx][sq_idx]
    }

    /// Returns the hash value for a hand piece
    pub fn hand_hash(&self, piece: Piece, count: u8) -> u64 {
        if count == 0 {
            return 0;
        }
        let pt_idx = Self::hand_piece_index(piece.piece_type).unwrap();
        let color_idx = Self::color_index(piece.color);
        let count_capped = (count as usize).min(19);
        self.hand_pieces[pt_idx][color_idx][count_capped]
    }

    /// Returns the hash value for side to move (XOR when White to move)
    pub fn side_hash(&self, color: Color) -> u64 {
        if color == Color::White { self.side_to_move } else { 0 }
    }

    /// XOR operation for toggling side to move
    pub fn toggle_side(&self) -> u64 {
        self.side_to_move
    }
}

impl Default for ZobristHash {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic() {
        // Same seed should produce same hash values
        let zh1 = ZobristHash::with_seed(12345);
        let zh2 = ZobristHash::with_seed(12345);

        let piece = Piece {
            piece_type: PieceType::Pawn,
            color: Color::Black,
        };
        let sq = Square::new(4, 6).unwrap();

        assert_eq!(zh1.piece_hash(piece, sq), zh2.piece_hash(piece, sq));
        assert_eq!(zh1.hand_hash(piece, 3), zh2.hand_hash(piece, 3));
        assert_eq!(zh1.side_hash(Color::White), zh2.side_hash(Color::White));
    }

    #[test]
    fn test_different_seeds() {
        // Different seeds should produce different hash values
        let zh1 = ZobristHash::with_seed(12345);
        let zh2 = ZobristHash::with_seed(54321);

        let piece = Piece {
            piece_type: PieceType::Pawn,
            color: Color::Black,
        };
        let sq = Square::new(4, 6).unwrap();

        assert_ne!(zh1.piece_hash(piece, sq), zh2.piece_hash(piece, sq));
    }

    #[test]
    fn test_side_to_move() {
        let zh = ZobristHash::new();

        // Black to move should return 0
        assert_eq!(zh.side_hash(Color::Black), 0);

        // White to move should return non-zero
        assert_ne!(zh.side_hash(Color::White), 0);

        // Toggle should return the same value as White to move
        assert_eq!(zh.toggle_side(), zh.side_hash(Color::White));
    }
}
