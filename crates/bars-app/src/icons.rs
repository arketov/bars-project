//! Иконки трея: скруглённый квадрат цвета состояния с буквой «b» (палитра «Малина», palette.md).
//! Рисуются в коде, отдельных файлов не нужно.

use bars_worker::worker::Health;
use tauri::image::Image;

const SIZE: u32 = 32;

fn color(health: Health, lit: bool) -> [u8; 3] {
    match health {
        Health::Idle => [0x8a, 0x85, 0x90],
        Health::Attention => [0xe8, 0xb1, 0x30],
        Health::Busy if lit => [0xd6, 0x33, 0x6c],
        // Вторая фаза мигания: тот же малиновый, но темнее.
        Health::Busy => [0x7a, 0x1d, 0x3d],
    }
}

fn inside_rounded_square(x: f32, y: f32) -> bool {
    let (edge, radius) = (SIZE as f32, 7.0_f32);
    let cx = x.clamp(radius, edge - radius);
    let cy = y.clamp(radius, edge - radius);
    (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
}

/// Буква «b»: ножка и чаша справа внизу.
fn inside_letter(x: f32, y: f32) -> bool {
    let stem = (9.0..13.0).contains(&x) && (6.0..26.0).contains(&y);
    let d = ((x - 17.0).powi(2) + (y - 19.0).powi(2)).sqrt();
    let bowl = (3.2..7.2).contains(&d) && x >= 12.0;
    stem || bowl
}

/// Иконка трея для состояния; `lit` — фаза мигания занятого воркера.
pub fn tray(health: Health, lit: bool) -> Image<'static> {
    let [r, g, b] = color(health, lit);
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for row in 0..SIZE {
        for column in 0..SIZE {
            let (x, y) = (column as f32 + 0.5, row as f32 + 0.5);
            if !inside_rounded_square(x, y) {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            } else if inside_letter(x, y) {
                rgba.extend_from_slice(&[255, 255, 255, 255]);
            } else {
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
    }
    Image::new_owned(rgba, SIZE, SIZE)
}
