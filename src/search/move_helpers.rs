use crate::board::{
    all_occ, bit, is_attacked, is_white_piece, move_ec, move_er, move_from, move_promotion,
    move_sc, move_sr, move_to, piece_on, piece_type, promotion_piece_index, see, BoardState, Move,
    BB, BK, BN, BP, BQ, BR, EMPTY_SQ, MATE, WB, WK, WN, WP, WQ, WR,
};
use crate::movegen::is_chess960_castling_move_mode;
use crate::tune::{self, TuneParam};

pub(super) fn piece_val(pt: u8) -> i32 {
    match pt {
        0 => 100,
        1 => 325,
        2 => 340,
        3 => 500,
        4 => 950,
        _ => 0,
    }
}

pub(super) fn piece_to_idx(pt: u8) -> usize {
    match pt {
        0 => 1,
        1 => 2,
        2 => 3,
        3 => 4,
        4 => 5,
        5 => 6,
        _ => 0,
    }
}

pub(super) fn from_to_key(sr: usize, sc: usize, er: usize, ec: usize) -> (usize, usize) {
    (sr * 8 + sc, er * 8 + ec)
}

pub(super) fn score_to_tt(score: i32, ply: usize) -> i32 {
    if score > MATE / 2 {
        score + ply as i32
    } else if score < -MATE / 2 {
        score - ply as i32
    } else {
        score
    }
}

pub(super) fn score_from_tt(score: i32, ply: usize) -> i32 {
    if score > MATE / 2 {
        score - ply as i32
    } else if score < -MATE / 2 {
        score + ply as i32
    } else {
        score
    }
}

#[inline]
pub(super) fn is_promotion_move(fpi: u8, mv: Move) -> bool {
    move_promotion(mv) != 0
        || (fpi != EMPTY_SQ && piece_type(fpi) == 0 && (move_er(mv) == 0 || move_er(mv) == 7))
}

pub(super) fn promotion_value(mv: Move) -> i32 {
    match move_promotion(mv).to_ascii_uppercase() {
        b'N' => piece_val(1),
        b'B' => piece_val(2),
        b'R' => piece_val(3),
        b'Q' => piece_val(4),
        _ => 0,
    }
}

#[inline]
pub(super) fn is_en_passant_capture(
    st: &BoardState,
    fpi: u8,
    mv: Move,
    to: usize,
    tpi: u8,
) -> bool {
    fpi != EMPTY_SQ
        && tpi == EMPTY_SQ
        && piece_type(fpi) == 0
        && Some(to) == st.ep
        && move_sc(mv) != move_ec(mv)
}

#[inline]
pub(super) fn capture_victim_value<const CHESS960: bool>(
    st: &BoardState,
    fpi: u8,
    mv: Move,
    to: usize,
    tpi: u8,
) -> i32 {
    if is_chess960_castling_move_mode::<CHESS960>(st, mv) {
        0
    } else if tpi != EMPTY_SQ {
        piece_val(piece_type(tpi))
    } else if is_en_passant_capture(st, fpi, mv, to, tpi) {
        piece_val(0)
    } else {
        0
    }
}

#[inline]
pub(super) fn move_is_capture<const CHESS960: bool>(
    st: &BoardState,
    fpi: u8,
    mv: Move,
    to: usize,
    tpi: u8,
) -> bool {
    !is_chess960_castling_move_mode::<CHESS960>(st, mv)
        && (tpi != EMPTY_SQ || is_en_passant_capture(st, fpi, mv, to, tpi))
}

#[inline]
pub(super) fn move_see<const CHESS960: bool>(
    st: &BoardState,
    mv: Move,
    from: usize,
    to: usize,
    fpi: u8,
    tpi: u8,
) -> i32 {
    if is_chess960_castling_move_mode::<CHESS960>(st, mv)
        || is_en_passant_capture(st, fpi, mv, to, tpi)
    {
        0
    } else {
        see(&st.bb, from, to)
    }
}

#[inline(always)]
pub(super) fn special_move_gives_check_mode<const CHESS960: bool>(
    st: &BoardState,
    mv: Move,
) -> bool {
    let from = move_from(mv);
    let to = move_to(mv);
    let fpi = st.mailbox[from];
    if fpi == EMPTY_SQ {
        return false;
    }

    let mut bb = st.bb;
    let mover_is_white = is_white_piece(fpi);
    let mover_type = piece_type(fpi);
    let is_chess960_castle = is_chess960_castling_move_mode::<CHESS960>(st, mv);
    let is_en_passant = mover_type == 0 && Some(to) == st.ep && move_sc(mv) != move_ec(mv);
    let is_standard_castle =
        mover_type == 5 && !CHESS960 && move_sc(mv) == 4 && (move_ec(mv) == 6 || move_ec(mv) == 2);

    if !is_en_passant && !is_chess960_castle && !is_standard_castle {
        return false;
    }

    if !is_chess960_castle {
        let tpi = piece_on(&bb, to);
        if tpi != EMPTY_SQ {
            bb[tpi as usize] &= !bit(to);
        }
    }

    if is_en_passant {
        let cap_sq = if mover_is_white { to + 8 } else { to - 8 };
        let ep_pi = piece_on(&bb, cap_sq);
        if ep_pi != EMPTY_SQ {
            bb[ep_pi as usize] &= !bit(cap_sq);
        }
    }

    if mover_type == 5 && is_chess960_castle {
        let rook_pi = if mover_is_white { WR } else { BR };
        let rook_col = move_ec(mv);
        let (king_dst_col, rook_dst_col) = if rook_col > move_sc(mv) {
            (6usize, 5usize)
        } else {
            (2usize, 3usize)
        };
        bb[rook_pi] &= !bit(move_er(mv) * 8 + rook_col);
        bb[rook_pi] |= bit(move_sr(mv) * 8 + rook_dst_col);
        bb[fpi as usize] &= !bit(from);
        bb[fpi as usize] |= bit(move_sr(mv) * 8 + king_dst_col);
    } else {
        bb[fpi as usize] &= !bit(from);

        if mover_type == 5
            && !CHESS960
            && move_sc(mv) == 4
            && (move_ec(mv) == 6 || move_ec(mv) == 2)
        {
            let rook_pi = if mover_is_white { WR } else { BR };
            let (rook_from, rook_to) = if move_ec(mv) == 6 {
                (move_sr(mv) * 8 + 7, move_sr(mv) * 8 + 5)
            } else {
                (move_sr(mv) * 8, move_sr(mv) * 8 + 3)
            };
            bb[rook_pi] &= !bit(rook_from);
            bb[rook_pi] |= bit(rook_to);
        }

        if mover_type == 0 && (move_er(mv) == 0 || move_er(mv) == 7) {
            if let Some(ppi) = promotion_piece_index(mover_is_white, move_promotion(mv)) {
                bb[ppi] |= bit(to);
            } else {
                bb[fpi as usize] |= bit(to);
            }
        } else {
            bb[fpi as usize] |= bit(to);
        }
    }

    let opponent_king = if st.w { bb[BK] } else { bb[WK] };
    opponent_king != 0 && is_attacked(&bb, opponent_king.trailing_zeros() as usize, st.w)
}

#[cfg(test)]
pub(super) fn special_move_gives_check(st: &BoardState, mv: Move) -> bool {
    if st.chess960 {
        special_move_gives_check_mode::<true>(st, mv)
    } else {
        special_move_gives_check_mode::<false>(st, mv)
    }
}

#[inline(always)]
fn pawn_protects(bb: &[u64; 12], sq: usize, is_white: bool) -> bool {
    let r = sq >> 3;
    let c = sq & 7;
    let (pawn_bb, guard_r) = if is_white {
        (bb[WP], r + 1)
    } else {
        (bb[BP], r.wrapping_sub(1))
    };
    if guard_r >= 8 {
        return false;
    }
    let mut mask = 0u64;
    if c > 0 {
        mask |= 1u64 << (guard_r * 8 + (c - 1));
    }
    if c < 7 {
        mask |= 1u64 << (guard_r * 8 + (c + 1));
    }
    pawn_bb & mask != 0
}

#[inline(always)]
fn square_attacked_by_enemy_pawns(bb: &[u64; 12], sq: usize, is_white: bool) -> bool {
    let r = sq >> 3;
    let c = sq & 7;
    let (enemy_pawn_bb, attack_r) = if is_white {
        (bb[BP], r.wrapping_sub(1))
    } else {
        (bb[WP], r + 1)
    };
    if attack_r >= 8 {
        return false;
    }
    let mut mask = 0u64;
    if c > 0 {
        mask |= 1u64 << (attack_r * 8 + (c - 1));
    }
    if c < 7 {
        mask |= 1u64 << (attack_r * 8 + (c + 1));
    }
    enemy_pawn_bb & mask != 0
}

#[inline(always)]
pub(super) fn king_shield_penalty(st: &BoardState, mv: Move) -> i32 {
    let from = move_from(mv);
    let fpi = st.mailbox[from];
    if fpi == EMPTY_SQ || piece_type(fpi) != 0 {
        return 0;
    }
    let is_white = is_white_piece(fpi);

    let ksq = st.king_sq(is_white);
    let kr = ksq >> 3;
    let kc = ksq & 7;
    let home_rank = if is_white { 7 } else { 0 };
    if kr != home_rank {
        return 0;
    }

    let fr = from >> 3;
    let fc = from & 7;
    if (fr as i32 - kr as i32).abs() > 1 || (fc as i32 - kc as i32).abs() > 1 {
        return 0;
    }

    let (opp_minors, opp_majors) = if is_white {
        (st.bb[BN] | st.bb[BB], st.bb[BR] | st.bb[BQ])
    } else {
        (st.bb[WN] | st.bb[WB], st.bb[WR] | st.bb[WQ])
    };
    if opp_minors == 0 && opp_majors == 0 {
        return 0;
    }

    -(tune::get_int(TuneParam::KingShieldPenalty, 4000) as i32)
}

#[inline(always)]
pub(super) fn outpost_bonus(st: &BoardState, mv: Move, safe_level: u8) -> i32 {
    let from = move_from(mv);
    let to = move_to(mv);
    let fpi = st.mailbox[from];
    if fpi == EMPTY_SQ {
        return 0;
    }
    let pt = piece_type(fpi);
    if pt != 1 && pt != 2 {
        return 0;
    }
    let is_white = is_white_piece(fpi);

    let r = to >> 3;
    let in_enemy_half = if is_white {
        (2..=4).contains(&r)
    } else {
        (3..=5).contains(&r)
    };
    if !in_enemy_half {
        return 0;
    }

    if !pawn_protects(&st.bb, to, is_white) {
        return 0;
    }

    if square_attacked_by_enemy_pawns(&st.bb, to, is_white) {
        return 0;
    }

    let bonus = tune::get_int(TuneParam::OutpostBonus, 3000) as i32;
    if safe_level >= 1 {
        let enemy_bishops = if is_white { st.bb[BB] } else { st.bb[WB] };
        if enemy_bishops != 0 {
            let occ = all_occ(&st.bb);
            let ba = crate::magic::bishop_attacks(to, occ);
            if enemy_bishops & ba != 0 {
                return 0;
            }
        }
    }
    if safe_level >= 2 {
        let enemy_knights = if is_white { st.bb[BN] } else { st.bb[WN] };
        if enemy_knights != 0 && enemy_knights & crate::board::KNIGHT_ATTACKS[to] != 0 {
            return 0;
        }
    }

    bonus
}

#[inline(always)]
pub(super) fn passed_push_bonus(st: &BoardState, mv: Move) -> i32 {
    let from = move_from(mv);
    let to = move_to(mv);
    let fpi = st.mailbox[from];
    if fpi == EMPTY_SQ || piece_type(fpi) != 0 {
        return 0;
    }
    let is_white = is_white_piece(fpi);

    let r = to >> 3;
    let is_target_rank = if is_white {
        r == 1 || r == 2
    } else {
        r == 4 || r == 5
    };
    if !is_target_rank {
        return 0;
    }

    let file = to & 7;
    let (_, enemy_pawns) = if is_white {
        (st.bb[WP], st.bb[BP])
    } else {
        (st.bb[BP], st.bb[WP])
    };

    let mut file_mask: u64 = 0;
    for f in file.saturating_sub(1)..=(file + 1).min(7) {
        file_mask |= 0x0101010101010101u64 << f;
    }
    let ahead = if is_white {
        let mut m = 0u64;
        for rr in 0..r {
            m |= 0xFFu64 << (rr * 8);
        }
        file_mask & m
    } else {
        let mut m = 0u64;
        for rr in (r + 1)..8 {
            m |= 0xFFu64 << (rr * 8);
        }
        file_mask & m
    };
    if enemy_pawns & ahead != 0 {
        return 0;
    }

    let npm = st.bb[WN]
        | st.bb[WB]
        | st.bb[WR]
        | st.bb[WQ]
        | st.bb[BN]
        | st.bb[BB]
        | st.bb[BR]
        | st.bb[BQ];
    if npm.count_ones() > 6 {
        return 0;
    }

    tune::get_int(TuneParam::PassedPushBonus, 3000) as i32
}

#[inline(always)]
pub(super) fn attack_order_bonus(st: &BoardState, mv: Move) -> i32 {
    let from = move_from(mv);
    let to = move_to(mv);
    let fpi = st.mailbox[from];
    if fpi == EMPTY_SQ {
        return 0;
    }
    let mover_is_white = is_white_piece(fpi);
    let mover_pt = piece_type(fpi);

    let tpi = st.mailbox[to];
    if tpi == EMPTY_SQ || is_white_piece(tpi) == mover_is_white {
        return 0;
    }
    let victim_pt = piece_type(tpi);
    if victim_pt == 0 || victim_pt == 5 {
        return 0;
    }

    if !piece_attacks_square(mover_pt, from, to, &st.bb) {
        return 0;
    }

    if is_attacked(&st.bb, from, !mover_is_white) {
        return 0;
    }

    let mover_val = piece_val(mover_pt);
    let victim_val = piece_val(victim_pt);
    if victim_val > mover_val {
        return tune::get_int(TuneParam::AttackOrderBonus, 4000) as i32;
    }

    if !square_defended_by(&st.bb, to, mover_is_white, from) {
        return tune::get_int(TuneParam::AttackOrderBonus, 4000) as i32;
    }

    0
}

#[inline(always)]
fn piece_attacks_square(pt: u8, from: usize, to: usize, bb: &[u64; 12]) -> bool {
    let occ = all_occ(bb);
    let to_bit = 1u64 << to;
    match pt {
        0 => {
            let r = from >> 3;
            let c = from & 7;
            let tr = to >> 3;
            let tc = to & 7;
            (tr as i32 - r as i32).abs() == 1 && (tc as i32 - c as i32).abs() == 1
        }
        1 => crate::board::KNIGHT_ATTACKS[from] & to_bit != 0,
        2 => crate::magic::bishop_attacks(from, occ) & to_bit != 0,
        3 => crate::magic::rook_attacks(from, occ) & to_bit != 0,
        4 => {
            (crate::magic::bishop_attacks(from, occ) | crate::magic::rook_attacks(from, occ))
                & to_bit
                != 0
        }
        _ => false,
    }
}

#[inline(always)]
fn square_defended_by(bb: &[u64; 12], sq: usize, is_white: bool, exclude_sq: usize) -> bool {
    let occ = all_occ(bb);
    if is_white {
        let r = sq >> 3;
        let c = sq & 7;
        if r < 7 {
            let rr = r + 1;
            let mut mask = 0u64;
            if c > 0 {
                let s = rr * 8 + (c - 1);
                if s != exclude_sq {
                    mask |= 1u64 << s;
                }
            }
            if c < 7 {
                let s = rr * 8 + (c + 1);
                if s != exclude_sq {
                    mask |= 1u64 << s;
                }
            }
            if bb[WP] & mask != 0 {
                return true;
            }
        }
    } else {
        let r = sq >> 3;
        let c = sq & 7;
        if r > 0 {
            let rr = r - 1;
            let mut mask = 0u64;
            if c > 0 {
                let s = rr * 8 + (c - 1);
                if s != exclude_sq {
                    mask |= 1u64 << s;
                }
            }
            if c < 7 {
                let s = rr * 8 + (c + 1);
                if s != exclude_sq {
                    mask |= 1u64 << s;
                }
            }
            if bb[BP] & mask != 0 {
                return true;
            }
        }
    }
    let (_, mut n, mut b, mut r, mut q, mut k) = if is_white {
        (bb[WP], bb[WN], bb[WB], bb[WR], bb[WQ], bb[WK])
    } else {
        (bb[BP], bb[BN], bb[BB], bb[BR], bb[BQ], bb[BK])
    };
    let exclude = bit(exclude_sq);
    n &= !exclude;
    b &= !exclude;
    r &= !exclude;
    q &= !exclude;
    k &= !exclude;
    if n & crate::board::KNIGHT_ATTACKS[sq] != 0 {
        return true;
    }
    if k & crate::board::KING_ATTACKS[sq] != 0 {
        return true;
    }
    let ba = crate::magic::bishop_attacks(sq, occ);
    if (b | q) & ba != 0 {
        return true;
    }
    let ra = crate::magic::rook_attacks(sq, occ);
    (r | q) & ra != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(st: &mut BoardState, pi: usize, sq: usize) {
        let is_white = is_white_piece(pi as u8);
        let pt = piece_type(pi as u8);
        let idx = if is_white {
            pt as usize
        } else {
            6 + pt as usize
        };
        st.bb[idx] |= bit(sq);
        st.mailbox[sq] = pi as u8;
    }

    #[test]
    fn king_shield_no_pawn_no_penalty() {
        let st = BoardState::empty();
        let mv = crate::board::encode_move(0, 4, 0, 5, 0);
        assert_eq!(king_shield_penalty(&st, mv), 0);
    }

    #[test]
    fn king_shield_pawn_shield_no_opponent_material_no_penalty() {
        let mut st = BoardState::empty();
        place(&mut st, WK, 60); // white king on e1
        place(&mut st, WP, 53); // white pawn on f2 (shield)
        let mv = crate::board::encode_move(6, 5, 6, 6, 0); // f2-f3
        assert_eq!(king_shield_penalty(&st, mv), 0);
    }

    #[test]
    fn king_shield_pawn_shield_with_opponent_material_penalty() {
        let mut st = BoardState::empty();
        place(&mut st, WK, 60); // white king on e1
        place(&mut st, WP, 53); // white pawn on f2 (shield)
        place(&mut st, BN, 44); // black knight on f3 (opponent non-pawn material)
        let mv = crate::board::encode_move(6, 5, 6, 6, 0); // f2-f3
        assert_eq!(
            king_shield_penalty(&st, mv),
            -(tune::get_int(TuneParam::KingShieldPenalty, 4000) as i32)
        );
    }

    #[test]
    fn king_shield_not_adjacent_no_penalty() {
        let mut st = BoardState::empty();
        place(&mut st, WK, 60); // white king on e1
        place(&mut st, WP, 50); // white pawn on c2 (not adjacent)
        place(&mut st, BN, 44); // black knight on f3
        let mv = crate::board::encode_move(6, 2, 6, 3, 0); // c2-d2
        assert_eq!(king_shield_penalty(&st, mv), 0);
    }

    #[test]
    fn king_shield_king_not_home_rank_no_penalty() {
        let mut st = BoardState::empty();
        place(&mut st, WK, 12); // white king on e6 (r=3)
        place(&mut st, WP, 5); // white pawn on f7
        place(&mut st, BN, 44); // black knight on f3
        let mv = crate::board::encode_move(0, 5, 0, 6, 0); // f7-g7
        assert_eq!(king_shield_penalty(&st, mv), 0);
    }

    #[test]
    fn outpost_knight_bonus() {
        let mut st = BoardState::empty();
        place(&mut st, WN, 45); // white knight on f3
        place(&mut st, WP, 35); // white pawn on d4 (protects e5)
        let mv = crate::board::encode_move(5, 5, 3, 4, 0); // f3-e5
        assert_eq!(
            outpost_bonus(&st, mv, 0),
            tune::get_int(TuneParam::OutpostBonus, 3000) as i32
        );
    }

    #[test]
    fn outpost_attacked_by_enemy_pawns_no_bonus() {
        let mut st = BoardState::empty();
        place(&mut st, WN, 45); // white knight on f3
        place(&mut st, WP, 35); // white pawn on d4 (protects e5)
        place(&mut st, BP, 19); // black pawn on d6 (attacks e5)
        let mv = crate::board::encode_move(5, 5, 3, 4, 0); // f3-e5
        assert_eq!(outpost_bonus(&st, mv, 0), 0);
    }

    #[test]
    fn outpost_not_protected_no_bonus() {
        let mut st = BoardState::empty();
        place(&mut st, WN, 45); // white knight on f3
        let mv = crate::board::encode_move(5, 5, 3, 4, 0); // f3-e5
        assert_eq!(outpost_bonus(&st, mv, 0), 0);
    }

    #[test]
    fn passed_push_bonus_endgame() {
        let mut st = BoardState::empty();
        place(&mut st, WP, 20); // white pawn on e6 (r=2, c=4)
        place(&mut st, WK, 60); // white king on e1
        place(&mut st, BK, 4); // black king on a8
                               // Very little non-pawn material: only kings
        let mv = crate::board::encode_move(2, 4, 1, 4, 0); // e6-e7
        assert_eq!(
            passed_push_bonus(&st, mv),
            tune::get_int(TuneParam::PassedPushBonus, 3000) as i32
        );
    }

    #[test]
    fn passed_push_not_endgame_no_bonus() {
        let mut st = BoardState::empty();
        place(&mut st, WP, 20); // white pawn on e6
        place(&mut st, WK, 60); // white king on e1
        place(&mut st, BK, 0); // black king on a8
                               // Lots of non-pawn material: full minor/major presence on both sides
        place(&mut st, WQ, 59); // white queen on d1
        place(&mut st, WR, 56); // white rook on a1
        place(&mut st, WB, 57); // white bishop on c1
        place(&mut st, WN, 58); // white knight on b1
        place(&mut st, BQ, 3); // black queen on d8
        place(&mut st, BR, 7); // black rook on h8
        place(&mut st, BB, 2); // black bishop on c8
        place(&mut st, BN, 1); // black knight on b8
        let mv = crate::board::encode_move(2, 4, 1, 4, 0); // e6-e7
        assert_eq!(passed_push_bonus(&st, mv), 0);
    }

    #[test]
    fn attack_order_heavier_victim_bonus() {
        let mut st = BoardState::empty();
        place(&mut st, WN, 45); // white knight on f3
        place(&mut st, BB, 28); // black bishop on e5
                                // Knight attacks e5; f3 not attacked by black; bishop heavier than knight
        let mv = crate::board::encode_move(5, 5, 3, 4, 0); // f3xe5
        assert_eq!(
            attack_order_bonus(&st, mv),
            tune::get_int(TuneParam::AttackOrderBonus, 4000) as i32
        );
    }

    #[test]
    fn attack_order_undefended_equal_victim_bonus() {
        let mut st = BoardState::empty();
        place(&mut st, WB, 58); // white bishop on c1 (square 58 = r=7,c=2)
        place(&mut st, BN, 37); // black knight on f4 (square 37 = r=4,c=5)
                                // Bishop attacks f4; c1 not attacked by black; f4 undefended by white
        let mv = crate::board::encode_move(7, 2, 4, 5, 0); // c1xf4
        assert_eq!(
            attack_order_bonus(&st, mv),
            tune::get_int(TuneParam::AttackOrderBonus, 4000) as i32
        );
    }

    #[test]
    fn attack_order_defended_no_bonus() {
        let mut st = BoardState::empty();
        place(&mut st, WN, 45); // white knight on f3
        place(&mut st, BN, 28); // black knight on e5
        place(&mut st, WP, 35); // white pawn on d4 defends e5 (white attacks toward decreasing r)
        let mv = crate::board::encode_move(5, 5, 3, 4, 0); // f3xe5
        assert_eq!(attack_order_bonus(&st, mv), 0);
    }

    #[test]
    fn attack_order_origin_attacked_no_bonus() {
        let mut st = BoardState::empty();
        place(&mut st, WN, 45); // white knight on f3
        place(&mut st, WB, 28); // black bishop on e5
                                // Black pawn on g4 attacks f3: from g4 (r=4,c=6) black captures to f5 (r=5,c=5) = f3
        place(&mut st, BP, 38); // black pawn on g4
        let mv = crate::board::encode_move(5, 5, 3, 4, 0); // f3xe5
        assert_eq!(attack_order_bonus(&st, mv), 0);
    }
}
