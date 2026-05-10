use windows::Win32::Foundation::POINT;

#[derive(Debug, Clone)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, point: &Point) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.w
            && point.y >= self.y
            && point.y <= self.y + self.h
    }
}

impl From<POINT> for Point {
    fn from(p: POINT) -> Self {
        Point {
            x: p.x as f32,
            y: p.y as f32,
        }
    }
}

impl From<Point> for POINT {
    fn from(p: Point) -> Self {
        POINT {
            x: p.x as i32,
            y: p.y as i32,
        }
    }
}
