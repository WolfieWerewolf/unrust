use webgl::*;
use webgl;

use image::RgbaImage;

use std::cell::RefCell;
use std::rc::Rc;
use engine::asset::{Asset, AssetResult, AssetSystem, FileFuture, LoadableAsset, Resource};
use std::path::Path;

#[derive(Debug)]
pub enum TextureFiltering {
    Nearest,
    Linear,
}

#[derive(Debug)]
enum TextureKind {
    Image(Resource<RgbaImage>),
    CubeMap([Resource<RgbaImage>; 6]),
    RenderTexture { size: (u32, u32) },
}

#[derive(Debug)]
pub struct Texture {
    pub filtering: TextureFiltering,
    gl_state: RefCell<Option<TextureGLState>>,
    kind: TextureKind,
}

pub enum TextureAsset {
    Single(Resource<RgbaImage>),
    Cube([Resource<RgbaImage>; 6]),
}

impl From<RgbaImage> for TextureAsset {
    fn from(img: RgbaImage) -> TextureAsset {
        TextureAsset::Single(Resource::new(img))
    }
}

impl Asset for Texture {
    type Resource = TextureAsset;

    fn new_from_resource(r: Self::Resource) -> Rc<Self> {
        return match r {
            TextureAsset::Single(res) => Rc::new(Texture {
                filtering: TextureFiltering::Linear,
                gl_state: RefCell::new(None),
                kind: TextureKind::Image(res),
            }),

            TextureAsset::Cube(res) => Rc::new(Texture {
                filtering: TextureFiltering::Linear,
                gl_state: RefCell::new(None),
                kind: TextureKind::CubeMap(res),
            }),
        };
    }
}

impl LoadableAsset for Texture {
    fn load<T: AssetSystem + Clone + 'static>(
        asys: &T,
        mut files: Vec<FileFuture>,
    ) -> Self::Resource {
        if files.len() == 6 {
            TextureAsset::Cube([
                Self::load_resource::<RgbaImage, T>(asys.clone(), files.remove(0)),
                Self::load_resource::<RgbaImage, T>(asys.clone(), files.remove(0)),
                Self::load_resource::<RgbaImage, T>(asys.clone(), files.remove(0)),
                Self::load_resource::<RgbaImage, T>(asys.clone(), files.remove(0)),
                Self::load_resource::<RgbaImage, T>(asys.clone(), files.remove(0)),
                Self::load_resource::<RgbaImage, T>(asys.clone(), files.remove(0)),
            ])
        } else {
            TextureAsset::Single(Self::load_resource::<RgbaImage, T>(
                asys.clone(),
                files.remove(0),
            ))
        }
    }

    fn gather<T: AssetSystem>(asys: &T, fname: &str) -> Vec<FileFuture> {
        let path = Path::new(fname);
        let ext = path.extension();
        let stem = path.file_stem();
        let parent = path.parent();
        let parent = parent.map_or("".to_string(), |p| p.to_str().unwrap().to_string() + "/");

        if ext.is_none() || stem.is_none() {
            return vec![asys.new_file(fname)];
        }

        let ext = ext.unwrap().to_str().unwrap();
        let stem = stem.unwrap().to_str().unwrap();
        let tag = "_cubemap";

        if stem.to_lowercase().ends_with(tag) {
            let f = (&stem[..stem.len() - tag.len()]).to_string();
            return vec![
                asys.new_file(&format!("{}{}_right.{}", &parent, &f, ext)),
                asys.new_file(&format!("{}{}_left.{}", &parent, &f, ext)),
                asys.new_file(&format!("{}{}_top.{}", &parent, &f, ext)),
                asys.new_file(&format!("{}{}_bottom.{}", &parent, &f, ext)),
                asys.new_file(&format!("{}{}_front.{}", &parent, &f, ext)),
                asys.new_file(&format!("{}{}_back.{}", &parent, &f, ext)),
            ];
        }

        vec![asys.new_file(fname)]
    }
}

#[derive(Debug)]
struct TextureGLState {
    tex: WebGLTexture,
}

impl Texture {
    pub fn new_render_texture(width: u32, height: u32) -> Rc<Self> {
        Rc::new(Texture {
            filtering: TextureFiltering::Linear,
            gl_state: RefCell::new(None),
            kind: TextureKind::RenderTexture {
                size: (width, height),
            },
        })
    }

    pub fn bind(&self, gl: &WebGLRenderingContext, unit: u32) -> AssetResult<()> {
        self.prepare(gl)?;

        let state_option = self.gl_state.borrow();
        let state = state_option.as_ref().unwrap();

        gl.active_texture(unit);
        match self.kind {
            TextureKind::CubeMap(_) => gl.bind_texture_cube(&state.tex),
            _ => gl.bind_texture(&state.tex),
        }

        Ok(())
    }

    pub fn prepare(&self, gl: &WebGLRenderingContext) -> AssetResult<()> {
        if self.gl_state.borrow().is_some() {
            return Ok(());
        }

        let new_state = Some(texture_bind_buffer(gl, &self.filtering, &self.kind)?);

        self.gl_state.replace(new_state);

        Ok(())
    }
}

fn bind_to_framebuffer(gl: &WebGLRenderingContext, tex: &WebGLTexture) {
    gl.framebuffer_texture2d(
        Buffers::Framebuffer,
        Buffers::ColorAttachment0,
        TextureBindPoint::Texture2d,
        tex,
        0,
    );
}

fn unbind_texture(gl: &WebGLRenderingContext, kind: &TextureKind) {
    match kind {
        &TextureKind::Image(_) | &TextureKind::RenderTexture { .. } => {
            gl.unbind_texture();
        }
        &TextureKind::CubeMap(_) => {
            gl.unbind_texture_cube();
        }
    }
}

fn texture_bind_buffer(
    gl: &WebGLRenderingContext,
    texfilter: &TextureFiltering,
    kind: &TextureKind,
) -> AssetResult<TextureGLState> {
    let mut gl_tex_kind: webgl::TextureKind = webgl::TextureKind::Texture2d;

    let tex = match kind {
        &TextureKind::Image(ref img_res) => {
            let img = img_res.try_into()?;

            let tex = gl.create_texture();
            gl.active_texture(0);
            gl.bind_texture(&tex);

            gl.tex_image2d(
                TextureBindPoint::Texture2d, // target
                0,                           // level
                img.width() as u16,          // width
                img.height() as u16,         // height
                PixelFormat::Rgba,           // format
                DataType::U8,                // type
                &*img,                       // data
            );

            tex
        }
        &TextureKind::CubeMap(ref img_res) => {
            let mut imgs = Vec::new();

            let bindpoints = [
                TextureBindPoint::TextureCubeMapPositiveX,
                TextureBindPoint::TextureCubeMapNegativeX,
                TextureBindPoint::TextureCubeMapPositiveY,
                TextureBindPoint::TextureCubeMapNegativeY,
                TextureBindPoint::TextureCubeMapPositiveZ,
                TextureBindPoint::TextureCubeMapNegativeZ,
            ];

            for res in img_res.iter() {
                imgs.push(res.try_borrow()?)
            }

            let tex = gl.create_texture();
            gl.active_texture(0);
            gl.bind_texture_cube(&tex);

            for (i, img) in imgs.iter().enumerate() {
                gl.tex_image2d(
                    bindpoints[i],       // target
                    0,                   // level
                    img.width() as u16,  // width
                    img.height() as u16, // height
                    PixelFormat::Rgba,   // format
                    DataType::U8,        // type
                    &*img,               // data
                );
            }

            gl_tex_kind = webgl::TextureKind::TextureCubeMap;

            tex
        }

        &TextureKind::RenderTexture { size } => {
            let tex = gl.create_texture();
            gl.active_texture(0);
            gl.bind_texture(&tex);
            gl.tex_image2d(
                TextureBindPoint::Texture2d, // target
                0,                           // level
                size.0 as u16,               // width
                size.1 as u16,               // height
                PixelFormat::Rgba,           // format
                DataType::U8,                // type
                &[],                         // data
            );

            tex
        }
    };

    let filtering: i32 = match texfilter {
        &TextureFiltering::Nearest => TextureMagFilter::Nearest as i32,
        _ => TextureMagFilter::Linear as i32,
    };

    gl.tex_parameteri(gl_tex_kind, TextureParameter::TextureMagFilter, filtering);
    gl.tex_parameteri(gl_tex_kind, TextureParameter::TextureMinFilter, filtering);
    gl.tex_parameteri(
        gl_tex_kind,
        TextureParameter::TextureWrapS,
        TextureWrap::ClampToEdge as i32,
    );
    gl.tex_parameteri(
        gl_tex_kind,
        TextureParameter::TextureWrapT,
        TextureWrap::ClampToEdge as i32,
    );

    if let &TextureKind::CubeMap(..) = kind {
        gl.tex_parameteri(
            gl_tex_kind,
            TextureParameter::TextureWrapR,
            TextureWrap::ClampToEdge as i32,
        );
    }

    if let &TextureKind::RenderTexture { .. } = kind {
        bind_to_framebuffer(gl, &tex);
    }

    unbind_texture(gl, kind);

    Ok(TextureGLState { tex: tex })
}
