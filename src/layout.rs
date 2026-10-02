//! Where pins, a collapsed group, and split panes sit.
//!
//! The widgets only apply these rectangles. A layout change is a move from
//! the previous rectangle to the next one.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub id: u64,
    pub members: Vec<u64>,
    pub collapsed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Split {
    Pair { left: u64, right: u64 },
    Grid { large: u64, stacked: [u64; 2], stack_left: bool },
}

impl Split {
    pub fn contains(&self, id: u64) -> bool {
        self.ids().contains(&id)
    }

    pub fn ids(&self) -> Vec<u64> {
        match self {
            Split::Pair { left, right } => vec![*left, *right],
            Split::Grid { large, stacked, .. } => vec![*large, stacked[0], stacked[1]],
        }
    }
}

/// One pin per row, each the full width of the column: a horizontal
/// rectangle rather than a square.
pub fn pin_cells(count: usize, room: f64, gap: f64, height: f64) -> Vec<Rect> {
    (0..count).map(|i| Rect { x: 0.0, y: i as f64 * (height + gap), w: room, h: height }).collect()
}

/// How many member icons a collapsed group shows, and whether a "+" follows.
pub fn group_face(count: usize) -> (usize, bool) {
    (count.min(3), count > 3)
}

/// Add a loose tab to the latest group, or open a group for it.
pub fn join_group(groups: &mut Vec<Group>, next_id: &mut u64, tab: u64) {
    if groups.iter().any(|group| group.members.contains(&tab)) {
        return;
    }
    if let Some(group) = groups.last_mut() {
        group.members.push(tab);
        return;
    }
    let id = *next_id;
    *next_id += 1;
    groups.push(Group { id, members: vec![tab], collapsed: false });
}

pub fn leave_group(groups: &mut Vec<Group>, tab: u64) {
    for group in groups.iter_mut() {
        group.members.retain(|id| *id != tab);
    }
    groups.retain(|group| !group.members.is_empty());
}

/// Drop `incoming` onto `side` of the page that is showing.
///
/// One page becomes a side-by-side pair. A pair becomes one large pane and
/// two stacked panes. A grid stays a grid.
pub fn drop_split(current: Option<&Split>, active: u64, incoming: u64, side: Side) -> Option<Split> {
    if incoming == active {
        return current.cloned();
    }
    if current.is_some_and(|split| split.contains(incoming)) {
        return current.cloned();
    }
    Some(match current {
        None => match side {
            Side::Left => Split::Pair { left: incoming, right: active },
            Side::Right => Split::Pair { left: active, right: incoming },
        },
        Some(Split::Pair { left, right }) => match side {
            Side::Left => Split::Grid { large: *right, stacked: [*left, incoming], stack_left: true },
            Side::Right => Split::Grid { large: *left, stacked: [*right, incoming], stack_left: false },
        },
        Some(grid) => grid.clone(),
    })
}

/// The split that remains after `id` goes away.
pub fn without(split: &Split, id: u64) -> Option<Split> {
    if !split.contains(id) {
        return Some(split.clone());
    }
    match *split {
        Split::Pair { .. } => None,
        Split::Grid { large, stacked, stack_left } => {
            if large == id {
                return Some(Split::Pair { left: stacked[0], right: stacked[1] });
            }
            let other = if stacked[0] == id { stacked[1] } else { stacked[0] };
            Some(if stack_left {
                Split::Pair { left: other, right: large }
            } else {
                Split::Pair { left: large, right: other }
            })
        }
    }
}

pub fn place_split(split: &Split, width: f64, height: f64, gap: f64) -> Vec<(u64, Rect)> {
    let width = width.max(gap + 2.0);
    let height = height.max(gap + 2.0);
    let half_w = ((width - gap) / 2.0).max(1.0);
    let half_h = ((height - gap) / 2.0).max(1.0);
    match split {
        Split::Pair { left, right } => vec![
            (*left, Rect { x: 0.0, y: 0.0, w: half_w, h: height }),
            (*right, Rect { x: half_w + gap, y: 0.0, w: half_w, h: height }),
        ],
        Split::Grid { large, stacked, stack_left } => {
            let stack_x = if *stack_left { 0.0 } else { half_w + gap };
            let large_x = if *stack_left { half_w + gap } else { 0.0 };
            vec![
                (*large, Rect { x: large_x, y: 0.0, w: half_w, h: height }),
                (stacked[0], Rect { x: stack_x, y: 0.0, w: half_w, h: half_h }),
                (stacked[1], Rect { x: stack_x, y: half_h + gap, w: half_w, h: half_h }),
            ]
        }
    }
}

/// The half of the page a dragged tab is over, once the pointer has left the sidebar.
/// In the top strip the pointer is reordering, not splitting.
#[allow(clippy::too_many_arguments)]
pub fn drop_side_at(
    sidebar: bool,
    side_right: bool,
    side_width: f64,
    window_w: f64,
    window_h: f64,
    strip: f64,
    x: f64,
    y: f64,
) -> Option<Side> {
    if y < 0.0 || y > window_h {
        return None;
    }
    if !sidebar && y < strip {
        return None;
    }
    let (left, right) = if !sidebar {
        (0.0, window_w)
    } else if side_right {
        (0.0, (window_w - side_width).max(0.0))
    } else {
        (side_width, window_w)
    };
    if x < left + 12.0 || x > right {
        return None;
    }
    Some(if x < (left + right) / 2.0 { Side::Left } else { Side::Right })
}

/// Move `id` to where `onto` sits inside one group.
pub fn reorder_member(members: &mut Vec<u64>, id: u64, onto: u64) -> bool {
    let Some(from) = members.iter().position(|member| *member == id) else { return false };
    let Some(to) = members.iter().position(|member| *member == onto) else { return false };
    if from == to {
        return false;
    }
    let member = members.remove(from);
    let to = if from < to { to - 1 } else { to };
    members.insert(to, member);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_pins_form_four_rows() {
        let cells = pin_cells(4, 200.0, 4.0, 28.0);
        println!("{cells:?}");
        assert_eq!(cells.len(), 4);
        assert_eq!(cells[0], Rect { x: 0.0, y: 0.0, w: 200.0, h: 28.0 });
        assert_eq!(cells[1], Rect { x: 0.0, y: 32.0, w: 200.0, h: 28.0 });
        assert_eq!(cells[3], Rect { x: 0.0, y: 96.0, w: 200.0, h: 28.0 });
        // Every pin is wider than it is tall, and none overlap.
        for (above, below) in cells.iter().zip(cells.iter().skip(1)) {
            assert!(above.w > above.h);
            assert!(below.y >= above.y + above.h);
        }
        assert_eq!(pin_cells(5, 200.0, 4.0, 28.0).len(), 5);
        assert_eq!(pin_cells(5, 200.0, 4.0, 28.0)[4].y, 128.0);
        assert!(pin_cells(0, 200.0, 4.0, 28.0).is_empty());
    }

    #[test]
    fn a_collapsed_group_stops_at_three_icons() {
        assert_eq!(group_face(0), (0, false));
        assert_eq!(group_face(3), (3, false));
        assert_eq!(group_face(4), (3, true));
        let mut groups = vec![];
        let mut next = 1;
        for tab in 1..=4 {
            join_group(&mut groups, &mut next, tab);
        }
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members, vec![1, 2, 3, 4]);
        leave_group(&mut groups, 2);
        assert_eq!(groups[0].members, vec![1, 3, 4]);
    }

    #[test]
    fn drops_build_a_pair_and_then_a_grid() {
        let pair = drop_split(None, 1, 2, Side::Left).unwrap();
        assert_eq!(pair, Split::Pair { left: 2, right: 1 });
        let right = drop_split(None, 1, 2, Side::Right).unwrap();
        assert_eq!(right, Split::Pair { left: 1, right: 2 });
        let grid = drop_split(Some(&pair), 1, 3, Side::Right).unwrap();
        assert_eq!(grid, Split::Grid { large: 2, stacked: [1, 3], stack_left: false });
        let rects = place_split(&grid, 308.0, 208.0, 8.0);
        println!("{rects:?}");
        assert_eq!(rects[0].1, Rect { x: 0.0, y: 0.0, w: 150.0, h: 208.0 });
        assert_eq!(rects[1].1, Rect { x: 158.0, y: 0.0, w: 150.0, h: 100.0 });
        assert_eq!(rects[2].1, Rect { x: 158.0, y: 108.0, w: 150.0, h: 100.0 });
        assert!(rects[0].1.w > rects[1].1.h);
        assert_eq!(without(&grid, 3).unwrap(), Split::Pair { left: 2, right: 1 });
        assert_eq!(without(&pair, 2), None);
    }

    #[test]
    fn a_strip_drag_is_not_a_split() {
        assert_eq!(drop_side_at(false, false, 232.0, 1000.0, 800.0, 52.0, 400.0, 20.0), None);
        assert_eq!(drop_side_at(false, false, 232.0, 1000.0, 800.0, 52.0, 800.0, 400.0), Some(Side::Right));
        assert_eq!(drop_side_at(true, false, 232.0, 1000.0, 800.0, 52.0, 100.0, 40.0), None);
        assert_eq!(drop_side_at(true, false, 232.0, 1000.0, 800.0, 52.0, 600.0, 400.0), Some(Side::Left));
        let mut members = vec![1, 2, 3];
        assert!(reorder_member(&mut members, 3, 1));
        assert_eq!(members, vec![3, 1, 2]);
    }
}
