//! Canvas rasterization over the VM's straight RGBA backing store.
//! Only the damaged region is converted to tiny-skia's premultiplied format.

use serde::{Deserialize, Serialize};
use tiny_skia::{BlendMode, FillRule, Mask, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};
use crate::inline::TextEngine;

pub type CanvasPath = Vec<Vec<f32>>;

#[derive(Deserialize)]
pub struct CanvasText {
    pub text: String,
    pub size: f32,
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub align: String,
    pub baseline: String,
    pub rtl: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMetrics {
    pub width: f32,
    pub actual_bounding_box_left: f32,
    pub actual_bounding_box_right: f32,
    pub actual_bounding_box_ascent: f32,
    pub actual_bounding_box_descent: f32,
    pub font_bounding_box_ascent: f32,
    pub font_bounding_box_descent: f32,
}

#[derive(Deserialize)]
#[serde(tag = "op")]
pub enum CanvasShape {
    #[serde(rename = "fill_rect")]
    FillRect { rect: [f32;4] },
    #[serde(rename = "clear_rect")]
    ClearRect { rect: [f32;4] },
    #[serde(rename = "stroke_rect")]
    StrokeRect { rect: [f32;4] },
    #[serde(rename = "fill")]
    Fill { path: CanvasPath, rule: String },
    #[serde(rename = "stroke")]
    Stroke { path: CanvasPath },
    #[serde(rename = "text")]
    Text { spec: CanvasText, x: f32, y: f32, max_width: Option<f32>, stroke: bool },
}

#[derive(Deserialize)]
pub struct CanvasClip {
    pub path: CanvasPath,
    pub rule: String,
}

#[derive(Deserialize)]
pub struct CanvasDraw {
    pub shape: CanvasShape,
    pub transform: [f32;6],
    pub color: String,
    pub alpha: f32,
    pub composite: String,
    pub line_width: f32,
    pub line_cap: String,
    pub line_join: String,
    pub miter_limit: f32,
    pub dash: Vec<f32>,
    pub dash_offset: f32,
    pub clips: Vec<CanvasClip>,
}

fn rule(value: &str) -> FillRule {
    if value == "evenodd" { FillRule::EvenOdd } else { FillRule::Winding }
}

fn path(commands: &CanvasPath) -> Result<Option<Path>, String> {
    if commands.len() > 16_384 { return Err("Canvas path exceeds the segment budget".into()); }
    let mut builder = PathBuilder::new();
    for cmd in commands {
        if !cmd.iter().all(|value| value.is_finite()) { return Err("Invalid canvas path".into()); }
        match cmd.as_slice() {
            [0.,x,y] => builder.move_to(*x,*y),
            [1.,x,y] => builder.line_to(*x,*y),
            [2.,x1,y1,x,y] => builder.quad_to(*x1,*y1,*x,*y),
            [3.,x1,y1,x2,y2,x,y] => builder.cubic_to(*x1,*y1,*x2,*y2,*x,*y),
            [4.] => builder.close(),
            _ => return Err("Invalid canvas path command".into()),
        }
    }
    Ok(builder.finish())
}

fn matrix(values: [f32;6]) -> Transform {
    Transform::from_row(values[0],values[1],values[2],values[3],values[4],values[5])
}

fn rectangle(rect: [f32;4], transform: Transform) -> Option<Path> {
    if !rect.iter().all(|value| value.is_finite()) { return None; }
    let [x,y,w,h] = rect;
    if w == 0. || h == 0. { return None; }
    let mut builder = PathBuilder::new();
    builder.move_to(x,y); builder.line_to(x+w,y);
    builder.line_to(x+w,y+h); builder.line_to(x,y+h); builder.close();
    builder.finish()?.transform(transform)
}

fn stroke(path: Path, draw: &CanvasDraw) -> Option<Path> {
    let transform = matrix(draw.transform);
    let local = path.transform(transform.invert()?)?;
    let mut stroke = Stroke {
        width: draw.line_width,
        miter_limit: draw.miter_limit,
        line_cap: match draw.line_cap.as_str() {
            "round" => tiny_skia::LineCap::Round,
            "square" => tiny_skia::LineCap::Square,
            _ => tiny_skia::LineCap::Butt,
        },
        line_join: match draw.line_join.as_str() {
            "round" => tiny_skia::LineJoin::Round,
            "bevel" => tiny_skia::LineJoin::Bevel,
            _ => tiny_skia::LineJoin::Miter,
        },
        ..Stroke::default()
    };
    if !draw.dash.is_empty() {
        stroke.dash = tiny_skia::StrokeDash::new(draw.dash.clone(), draw.dash_offset);
    }
    local.stroke(&stroke, 1.)?.transform(transform)
}

fn shape_text(
    engine: &mut Option<Box<TextEngine>>,
    spec: &CanvasText,
) -> Result<(TextMetrics, Option<Path>), String> {
    if spec.text.len() > 16_384 || !spec.size.is_finite() || spec.size <= 0. || spec.size > 4096. {
        return Err("Canvas text exceeds the shaping budget".into());
    }
    let engine = engine.get_or_insert_with(|| Box::new(TextEngine::new()));
    let (width, ascent, descent, path) = engine.canvas_text_outline(
        &spec.text, spec.size, &spec.family, spec.bold, spec.italic,
    );
    let dx = match spec.align.as_str() {
        "center" => -width/2.,
        "right" => -width,
        "start" if spec.rtl => -width,
        "end" if !spec.rtl => -width,
        _ => 0.,
    };
    let dy = match spec.baseline.as_str() {
        "top" => ascent,
        "hanging" => ascent * 0.8,
        "middle" => (ascent-descent)/2.,
        "bottom" | "ideographic" => -descent,
        _ => 0.,
    };
    let path = path.and_then(|path| path.transform(Transform::from_translate(dx,dy)));
    let bounds = path.as_ref().map(Path::bounds);
    Ok((TextMetrics {
        width,
        actual_bounding_box_left: bounds.map_or(0., |b| -b.left()),
        actual_bounding_box_right: bounds.map_or(0., |b| b.right()),
        actual_bounding_box_ascent: bounds.map_or(0., |b| -b.top()),
        actual_bounding_box_descent: bounds.map_or(0., |b| b.bottom()),
        font_bounding_box_ascent: ascent-dy,
        font_bounding_box_descent: descent+dy,
    }, path))
}

pub fn measure_text(engine: &mut Option<Box<TextEngine>>, spec: &CanvasText) -> Result<TextMetrics, String> {
    shape_text(engine, spec).map(|(metrics,_)| metrics)
}

pub fn draw(
    rgba: &mut [u8], width: u32, height: u32,
    engine: &mut Option<Box<TextEngine>>, draw: &CanvasDraw,
) -> Result<(), String> {
    if width == 0 || height == 0 { return Ok(()); }
    if rgba.len() != width as usize * height as usize * 4
        || !draw.transform.iter().all(|v| v.is_finite())
        || !draw.alpha.is_finite() || !(0. ..=1.).contains(&draw.alpha)
        || !draw.line_width.is_finite() || draw.line_width <= 0.
        || !draw.miter_limit.is_finite() || draw.miter_limit <= 0.
        || !draw.dash_offset.is_finite() || draw.dash.len() > 1024
        || draw.dash.iter().any(|v| !v.is_finite() || *v < 0.) || draw.clips.len() > 64 {
        return Err("Invalid canvas drawing state".into());
    }
    let transform = matrix(draw.transform);
    let (path, fill_rule, clear) = match &draw.shape {
        CanvasShape::FillRect { rect } => (rectangle(*rect,transform),FillRule::Winding,false),
        CanvasShape::ClearRect { rect } => (rectangle(*rect,transform),FillRule::Winding,true),
        CanvasShape::StrokeRect { rect } => (rectangle(*rect,transform).and_then(|p| stroke(p,draw)),FillRule::Winding,false),
        CanvasShape::Fill { path: cmds, rule: fill_rule } => (path(cmds)?,rule(fill_rule),false),
        CanvasShape::Stroke { path: cmds } => (path(cmds)?.and_then(|p| stroke(p,draw)),FillRule::Winding,false),
        CanvasShape::Text { spec, x, y, max_width, stroke: outlined } => {
            if !x.is_finite() || !y.is_finite() { return Ok(()); }
            if max_width.is_some_and(|w| !w.is_finite() || w <= 0.) { return Ok(()); }
            let (metrics,path) = shape_text(engine,spec)?;
            let scale = max_width.filter(|w| *w < metrics.width).map_or(1.,|w| w/metrics.width);
            let local = Transform::from_row(scale,0.,0.,1.,*x,*y);
            let path = path.and_then(|p| p.transform(local)).and_then(|p| p.transform(transform));
            (if *outlined { path.and_then(|p| stroke(p,draw)) } else { path },FillRule::Winding,false)
        }
    };
    let Some(path) = path else { return Ok(()) };
    let bounds = path.bounds();
    let x0 = bounds.left().floor().clamp(0.,width as f32) as u32;
    let y0 = bounds.top().floor().clamp(0.,height as f32) as u32;
    let x1 = bounds.right().ceil().clamp(0.,width as f32) as u32;
    let y1 = bounds.bottom().ceil().clamp(0.,height as f32) as u32;
    if x0 >= x1 || y0 >= y1 { return Ok(()) }
    let color = crate::style::parse_color(&draw.color).unwrap_or([0,0,0,255]);
    let alpha = (color[3] as f32 * draw.alpha).round() as u8;
    // Common board/chart fast path: no full-surface copy or mask allocation.
    let integral = bounds.left().fract() == 0. && bounds.top().fract() == 0.
        && bounds.right().fract() == 0. && bounds.bottom().fract() == 0.;
    let axis_aligned = draw.transform[1] == 0. && draw.transform[2] == 0.;
    if draw.clips.is_empty() && integral && axis_aligned
        && (matches!(&draw.shape,CanvasShape::FillRect {..}) && alpha == 255 && draw.composite == "source-over" || clear) {
        let pixel = if clear { [0;4] } else { [color[0],color[1],color[2],255] };
        for y in y0..y1 {
            let start = ((y*width+x0)*4) as usize;
            let end = ((y*width+x1)*4) as usize;
            for dst in rgba[start..end].chunks_exact_mut(4) { dst.copy_from_slice(&pixel); }
        }
        return Ok(());
    }
    let (w,h) = (x1-x0,y1-y0);
    let mut pixmap = Pixmap::new(w,h).ok_or("Canvas raster allocation failed")?;
    for y in 0..h {
        for x in 0..w {
            let from = (((y+y0)*width+x+x0)*4) as usize;
            let to = ((y*w+x)*4) as usize;
            let a = rgba[from+3] as u32;
            for c in 0..3 { pixmap.data_mut()[to+c] = ((rgba[from+c] as u32*a+127)/255) as u8; }
            pixmap.data_mut()[to+3] = a as u8;
        }
    }
    let offset = Transform::from_translate(-(x0 as f32),-(y0 as f32));
    let mut mask = None;
    for clip in &draw.clips {
        let clip_path = path_from_clip(clip)?;
        let first = mask.is_none();
        let mask = mask.get_or_insert_with(|| Mask::new(w,h).unwrap());
        if let Some(path) = clip_path {
            if first {
                // The first clip initializes the intersection; subsequent
                // clips always intersect, including an empty intersection.
                mask.fill_path(&path,rule(&clip.rule),true,offset);
            } else {
                mask.intersect_path(&path,rule(&clip.rule),true,offset);
            }
        } else { mask.data_mut().fill(0); }
    }
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0],color[1],color[2],alpha);
    paint.anti_alias = true;
    paint.blend_mode = if clear { BlendMode::Clear } else {
        match draw.composite.as_str() {
            "multiply" => BlendMode::Multiply,
            "screen" => BlendMode::Screen,
            "destination-over" => BlendMode::DestinationOver,
            _ => BlendMode::SourceOver,
        }
    };
    pixmap.fill_path(&path,&paint,fill_rule,offset,mask.as_ref());
    for y in 0..h {
        for x in 0..w {
            let to = (((y+y0)*width+x+x0)*4) as usize;
            let from = ((y*w+x)*4) as usize;
            let pixel = &pixmap.data()[from..from+4];
            let old_a = rgba[to+3] as u32;
            let same = pixel[3] as u32 == old_a && (0..3).all(|c|
                pixel[c] as u32 == (rgba[to+c] as u32*old_a+127)/255);
            if same { continue; }
            let a = pixel[3] as u32;
            for c in 0..3 { rgba[to+c] = if a == 0 { 0 } else { ((pixel[c] as u32*255+a/2)/a).min(255) as u8 }; }
            rgba[to+3] = a as u8;
        }
    }
    Ok(())
}

fn path_from_clip(clip: &CanvasClip) -> Result<Option<Path>, String> { path(&clip.path) }
