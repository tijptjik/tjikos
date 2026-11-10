//! Small stroke icons, independent of the host's icon theme.
use tjik_gtk::gtk::{self, prelude::*};
pub fn icon(name: &'static str) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .content_width(22)
        .content_height(22)
        .build();
    area.add_css_class("stroke-icon");
    area.set_draw_func(move |area, c, width, height| {
        c.translate((width as f64 - 22.) / 2., (height as f64 - 22.) / 2.);
        let ink = area.color();
        c.set_source_rgba(
            ink.red() as f64,
            ink.green() as f64,
            ink.blue() as f64,
            ink.alpha() as f64,
        );
        c.set_line_width(1.5);
        c.set_line_cap(gtk::cairo::LineCap::Round);
        match name {
            "search" => {
                c.arc(9., 9., 6., 0., std::f64::consts::TAU);
                let _ = c.stroke();
                c.move_to(14., 14.);
                c.line_to(20., 20.);
            }
            "completed" | "cancelled" | "details" => {
                c.arc(11., 11., 8., 0., std::f64::consts::TAU);
                let _ = c.stroke();
                match name {
                    "completed" => {
                        c.move_to(6., 11.);
                        c.line_to(10., 15.);
                        c.line_to(16., 7.);
                    }
                    "cancelled" => {
                        c.move_to(7., 7.);
                        c.line_to(15., 15.);
                        c.move_to(15., 7.);
                        c.line_to(7., 15.);
                    }
                    _ => {
                        c.move_to(11., 10.);
                        c.line_to(11., 16.);
                        c.move_to(11., 6.);
                        c.line_to(11., 6.5);
                    }
                }
            }
            "blocked" => {
                c.rectangle(5., 9., 12., 11.);
                c.move_to(7., 9.);
                c.line_to(7., 6.);
                c.curve_to(7., 2., 15., 2., 15., 6.);
                c.line_to(15., 9.);
                c.move_to(11., 12.);
                c.line_to(11., 16.);
            }
            "undo" => {
                c.translate(-0.5, -1.);
                c.move_to(9., 4.);
                c.line_to(3., 10.);
                c.line_to(9., 15.);
                c.move_to(3., 10.);
                c.line_to(14., 10.);
                c.curve_to(23., 10., 22., 20., 13., 20.);
            }
            "edit" => {
                c.translate(0.5, -1.5);
                c.move_to(4., 15.);
                c.line_to(14., 5.);
                c.line_to(18., 9.);
                c.line_to(8., 19.);
                c.line_to(3., 20.);
                c.close_path();
                c.move_to(12., 7.);
                c.line_to(16., 11.);
            }
            "settings" => {
                c.arc(11., 11., 6., 0., std::f64::consts::TAU);
                let _ = c.stroke();
                c.arc(11., 11., 2.5, 0., std::f64::consts::TAU);
                let _ = c.stroke();
                for i in 0..8 {
                    let angle = i as f64 * std::f64::consts::TAU / 8.;
                    c.move_to(11. + 7. * angle.cos(), 11. + 7. * angle.sin());
                    c.line_to(11. + 10. * angle.cos(), 11. + 10. * angle.sin());
                }
            }
            "keyboard" => {
                c.rectangle(1., 4., 20., 14.);
                for y in [7., 10.] {
                    for x in [4., 8., 12., 16.] {
                        c.move_to(x, y);
                        c.line_to(x + 1., y);
                    }
                }
                c.move_to(6., 14.);
                c.line_to(16., 14.);
            }
            _ => {}
        }
        let _ = c.stroke();
    });
    area
}
