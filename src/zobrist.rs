use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::{Piece, PieceType, Square};

/// Zobrist hash value type.
pub type ZobristHash = u64;

/// Number of piece types (14)
const NUM_PIECE_TYPES: usize = 14;
/// Number of colors (2)
const NUM_COLORS: usize = 2;
/// Number of squares (81)
const NUM_SQUARES: usize = 81;
/// Number of hand piece types (7: Pawn, Lance, Knight, Silver, Gold, Rook, Bishop)
const NUM_HAND_PIECE_TYPES: usize = 7;
/// Maximum count of hand pieces (18 pawns max + 1 for 0)
const MAX_HAND_COUNT: usize = 19;

/// Zobrist hash table for board pieces: [piece_type][color][square]
static mut BOARD_HASH: [[[u64; NUM_SQUARES]; NUM_COLORS]; NUM_PIECE_TYPES] =
    [[[0; NUM_SQUARES]; NUM_COLORS]; NUM_PIECE_TYPES];

/// Zobrist hash for side to move (applied when White to move)
static mut SIDE_TO_MOVE_HASH: u64 = 0;

/// Zobrist hash table for hand pieces: [piece_type_index][color][count]
static mut HAND_HASH: [[[u64; MAX_HAND_COUNT]; NUM_COLORS]; NUM_HAND_PIECE_TYPES] =
    [[[0; MAX_HAND_COUNT]; NUM_COLORS]; NUM_HAND_PIECE_TYPES];

/// Fixed seed for reproducible Zobrist hash values.
const ZOBRIST_SEED: u64 = 0x5408_12DB_8157_5EED;

/// Initializes the Zobrist hash tables. Must be called before using Zobrist hashing.
/// Uses ChaCha8 CSPRNG with a fixed seed for high-quality, reproducible random values.
pub fn init() {
    let mut rng = ChaCha8Rng::seed_from_u64(ZOBRIST_SEED);

    unsafe {
        // Initialize board hash
        for pt in 0..NUM_PIECE_TYPES {
            for c in 0..NUM_COLORS {
                for sq in 0..NUM_SQUARES {
                    BOARD_HASH[pt][c][sq] = rng.gen();
                }
            }
        }

        // Initialize side to move hash
        SIDE_TO_MOVE_HASH = rng.gen();

        // Initialize hand hash
        for pt in 0..NUM_HAND_PIECE_TYPES {
            for c in 0..NUM_COLORS {
                for count in 0..MAX_HAND_COUNT {
                    HAND_HASH[pt][c][count] = rng.gen();
                }
            }
        }
    }
}

/// Returns the hash value for a piece on a square.
#[inline(always)]
pub fn board_hash(piece: Piece, sq: Square) -> u64 {
    unsafe { BOARD_HASH[piece.piece_type.index()][piece.color.index()][sq.index()] }
}

/// Returns the hash value for side to move.
#[inline(always)]
pub fn side_to_move_hash() -> u64 {
    unsafe { SIDE_TO_MOVE_HASH }
}

/// Returns the hash value for hand pieces.
#[inline(always)]
pub fn hand_hash(piece: Piece, count: u8) -> u64 {
    if let Some(idx) = hand_piece_index(piece.piece_type) {
        unsafe { HAND_HASH[idx][piece.color.index()][count as usize] }
    } else {
        0
    }
}

/// Convert piece type to hand piece index.
#[inline(always)]
fn hand_piece_index(pt: PieceType) -> Option<usize> {
    match pt {
        PieceType::Pawn => Some(0),
        PieceType::Lance => Some(1),
        PieceType::Knight => Some(2),
        PieceType::Silver => Some(3),
        PieceType::Gold => Some(4),
        PieceType::Rook => Some(5),
        PieceType::Bishop => Some(6),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::square::consts::*;
    use crate::Color;

    #[test]
    fn test_zobrist_init() {
        init();

        // Verify different pieces on same square have different hashes
        let black_pawn = Piece {
            piece_type: PieceType::Pawn,
            color: Color::Black,
        };
        let white_pawn = Piece {
            piece_type: PieceType::Pawn,
            color: Color::White,
        };
        let black_rook = Piece {
            piece_type: PieceType::Rook,
            color: Color::Black,
        };

        assert_ne!(board_hash(black_pawn, SQ_5E), board_hash(white_pawn, SQ_5E));
        assert_ne!(board_hash(black_pawn, SQ_5E), board_hash(black_rook, SQ_5E));

        // Verify same piece on different squares have different hashes
        assert_ne!(board_hash(black_pawn, SQ_5E), board_hash(black_pawn, SQ_5F));

        // Verify hand hashes are different for different counts
        assert_ne!(hand_hash(black_pawn, 0), hand_hash(black_pawn, 1));
        assert_ne!(hand_hash(black_pawn, 1), hand_hash(black_pawn, 2));
    }
}
