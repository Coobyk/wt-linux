//! The split-pane layout tree: `Pane | VSplit(a, b) | HSplit(a, b)`.
//! `VSplit` puts `a` left of `b` (vertical divider); `HSplit` puts `a` above `b`.

pub type PaneId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDir {
    /// Side by side (`VSplit`).
    Vertical,
    /// Stacked (`HSplit`).
    Horizontal,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    /// Stable id for the divider's key and drag target.
    pub id: u32,
    /// Share of the space `a` gets, 0.1..=0.9.
    pub ratio: f32,
    pub a: Box<Layout>,
    pub b: Box<Layout>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Layout {
    Pane(PaneId),
    VSplit(Split),
    HSplit(Split),
}

pub const MIN_RATIO: f32 = 0.1;

impl Layout {
    /// Pane ids in reading order (left-to-right, top-to-bottom).
    pub fn panes(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<PaneId>) {
        match self {
            Layout::Pane(p) => out.push(*p),
            Layout::VSplit(s) | Layout::HSplit(s) => {
                s.a.collect(out);
                s.b.collect(out);
            }
        }
    }

    pub fn contains(&self, pane: PaneId) -> bool {
        self.panes().contains(&pane)
    }

    /// Replaces `target` with a split of `target` and `new`, `new` second. False if absent.
    pub fn split(&mut self, target: PaneId, dir: SplitDir, new: PaneId, split_id: u32) -> bool {
        match self {
            Layout::Pane(p) if *p == target => {
                let s = Split {
                    id: split_id,
                    ratio: 0.5,
                    a: Box::new(Layout::Pane(target)),
                    b: Box::new(Layout::Pane(new)),
                };
                *self = match dir {
                    SplitDir::Vertical => Layout::VSplit(s),
                    SplitDir::Horizontal => Layout::HSplit(s),
                };
                true
            }
            Layout::Pane(_) => false,
            Layout::VSplit(s) | Layout::HSplit(s) => {
                s.a.split(target, dir, new, split_id) || s.b.split(target, dir, new, split_id)
            }
        }
    }

    /// Removes `target`; its sibling takes the parent split's place. False if `target` is
    /// absent or is the only pane (the caller closes the tab instead).
    pub fn remove(&mut self, target: PaneId) -> bool {
        let (Layout::VSplit(s) | Layout::HSplit(s)) = self else {
            return false;
        };
        let keep = if *s.a == Layout::Pane(target) {
            Some(std::mem::replace(&mut *s.b, Layout::Pane(0)))
        } else if *s.b == Layout::Pane(target) {
            Some(std::mem::replace(&mut *s.a, Layout::Pane(0)))
        } else {
            None
        };
        match keep {
            Some(k) => {
                *self = k;
                true
            }
            None => s.a.remove(target) || s.b.remove(target),
        }
    }

    pub fn split_mut(&mut self, id: u32) -> Option<&mut Split> {
        match self {
            Layout::Pane(_) => None,
            Layout::VSplit(s) | Layout::HSplit(s) => {
                if s.id == id {
                    return Some(s);
                }
                if let Some(x) = s.a.split_mut(id) {
                    return Some(x);
                }
                s.b.split_mut(id)
            }
        }
    }

    pub fn set_ratio(&mut self, id: u32, ratio: f32) -> bool {
        match self.split_mut(id) {
            Some(s) => {
                s.ratio = ratio.clamp(MIN_RATIO, 1.0 - MIN_RATIO);
                true
            }
            None => false,
        }
    }

    /// The pane `step` places after `from` in reading order, wrapping.
    pub fn cycle(&self, from: PaneId, step: isize) -> PaneId {
        let panes = self.panes();
        let n = panes.len() as isize;
        let at = panes.iter().position(|p| *p == from).unwrap_or(0) as isize;
        panes[(at + step).rem_euclid(n) as usize]
    }

    /// Each pane's rect as fractions of the whole: `(pane, x, y, w, h)`.
    pub fn rects(&self) -> Vec<(PaneId, f32, f32, f32, f32)> {
        let mut out = Vec::new();
        self.rects_in(0.0, 0.0, 1.0, 1.0, &mut out);
        out
    }

    fn rects_in(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        out: &mut Vec<(PaneId, f32, f32, f32, f32)>,
    ) {
        match self {
            Layout::Pane(p) => out.push((*p, x, y, w, h)),
            Layout::VSplit(s) => {
                s.a.rects_in(x, y, w * s.ratio, h, out);
                s.b.rects_in(x + w * s.ratio, y, w * (1.0 - s.ratio), h, out);
            }
            Layout::HSplit(s) => {
                s.a.rects_in(x, y, w, h * s.ratio, out);
                s.b.rects_in(x, y + h * s.ratio, w, h * (1.0 - s.ratio), out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_remove() {
        let mut l = Layout::Pane(1);
        assert!(l.split(1, SplitDir::Vertical, 2, 10));
        assert!(l.split(2, SplitDir::Horizontal, 3, 11));
        assert_eq!(l.panes(), vec![1, 2, 3]);
        assert!(matches!(&l, Layout::VSplit(s) if matches!(*s.b, Layout::HSplit(_))));
        assert!(!l.split(9, SplitDir::Vertical, 4, 12));
        let r = l.rects();
        assert_eq!(r[0], (1, 0.0, 0.0, 0.5, 1.0));
        assert_eq!(r[2], (3, 0.5, 0.5, 0.5, 0.5));
        assert!(l.remove(2));
        assert_eq!(l.panes(), vec![1, 3]);
        assert!(l.remove(1));
        assert_eq!(l, Layout::Pane(3));
        assert!(!l.remove(3));
    }

    #[test]
    fn ratio_and_cycle() {
        let mut l = Layout::Pane(1);
        l.split(1, SplitDir::Vertical, 2, 7);
        assert!(l.set_ratio(7, 0.99));
        assert_eq!(l.split_mut(7).unwrap().ratio, 0.9);
        assert!(!l.set_ratio(8, 0.3));
        assert_eq!(l.cycle(2, 1), 1);
        assert_eq!(l.cycle(1, -1), 2);
    }
}
