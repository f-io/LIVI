#![allow(dead_code)]

#[derive(Debug)]
struct Node {
    x: usize,
    y: usize,
    width: usize,
}

/// Represents an atlas for packing rectangles.
#[derive(Debug)]
pub struct Atlas {
    width: usize,
    height: usize,
    nodes: Vec<Node>,
}

impl Atlas {
    /// Creates a new atlas with the specified width and height.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            nodes: vec![Node { x: 0, y: 0, width }],
        }
    }

    /// Returns the size (width and height) of the atlas.
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// Expands the atlas to the specified width and height.
    pub fn expand(&mut self, width: usize, height: usize) {
        // Insert node for empty space

        if width > self.width {
            self.insert_node(self.nodes.len(), self.width, 0, width - self.width);
        }

        self.width = width;
        self.height = height;
    }

    /// Resets the atlas to the specified width and height.
    pub fn reset(&mut self, width: usize, height: usize) {
        *self = Self::new(width, height);
    }

    /// Adds a rectangle with the specified width and height to the atlas.
    ///
    /// Returns the position (x, y) of the added rectangle, or `None` if the rectangle cannot be added.
    pub fn add_rect(&mut self, rect_width: usize, rect_height: usize) -> Option<(usize, usize)> {
        let mut besth = self.height;
        let mut bestw = self.width;
        let mut besti = None;
        let mut bestx = 0;
        let mut besty = 0;

        // Bottom left fit heuristic.
        for i in 0..self.nodes.len() {
            if let Some(y) = self.rect_fits(i, rect_width, rect_height) {
                if y + rect_height < besth || (y + rect_height == besth && self.nodes[i].width < bestw) {
                    besti = Some(i);
                    bestw = self.nodes[i].width;
                    besth = y + rect_height;
                    bestx = self.nodes[i].x;
                    besty = y;
                }
            }
        }

        if let Some(besti) = besti {
            // Perform the actual packing.
            self.add_skyline_level(besti, bestx, besty, rect_width, rect_height);
            return Some((bestx, besty));
        }

        None
    }

    fn insert_node(&mut self, idx: usize, x: usize, y: usize, width: usize) {
        self.nodes.insert(idx, Node { x, y, width });
    }

    fn remove_node(&mut self, idx: usize) {
        self.nodes.remove(idx);
    }

    fn add_skyline_level(&mut self, idx: usize, x: usize, y: usize, width: usize, height: usize) {
        // Insert new node
        self.insert_node(idx, x, y + height, width);

        // Delete skyline segments that fall under the shadow of the new segment.
        let mut i = idx + 1;

        while i < self.nodes.len() {
            if self.nodes[i].x < self.nodes[i - 1].x + self.nodes[i - 1].width {
                let shrink = self.nodes[i - 1].x + self.nodes[i - 1].width - self.nodes[i].x;

                self.nodes[i].x += shrink;
                let new_width = self.nodes[i].width as isize - shrink as isize;

                if new_width <= 0 {
                    self.remove_node(i);
                    i -= 1;
                } else {
                    self.nodes[i].width = new_width as usize;
                    break;
                }
            } else {
                break;
            }

            i += 1;
        }

        // Merge same height skyline segments that are next to each other.
        let mut i = 0isize;

        while i < self.nodes.len() as isize - 1 {
            let index = i as usize;

            if self.nodes[index].y == self.nodes[index + 1].y {
                self.nodes[index].width += self.nodes[index + 1].width;
                self.remove_node(index + 1);

                i -= 1;
            }

            i += 1;
        }
    }

    fn rect_fits(&self, mut idx: usize, width: usize, height: usize) -> Option<usize> {
        // Checks if there is enough space at the location of skyline span 'i',
        // and return the max height of all skyline spans under that at that location,
        // (think tetris block being dropped at that position). Or -1 if no space found.

        let x = self.nodes[idx].x;
        let mut y = self.nodes[idx].y;

        if x + width > self.width {
            return None;
        }

        let mut space_left = width as isize;

        while space_left > 0 {
            if idx == self.nodes.len() {
                return None;
            }

            y = y.max(self.nodes[idx].y);

            if y + height > self.height {
                return None;
            }

            space_left -= self.nodes[idx].width as isize;
            idx += 1;
        }

        Some(y)
    }
}
