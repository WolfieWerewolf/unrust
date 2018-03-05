use std::rc::Rc;
use std::rc;
use std::cell::{Ref, RefCell, RefMut};
use std::sync::Arc;
use std::sync;

use world::app_fs::AppEngine;
use engine::{AssetSystem, Camera, ClearOption, Component, ComponentBased, ComponentEvent, Engine,
             GameObject, IEngine, SceneTree};

use engine::imgui;

use uni_app::{now, App, AppConfig, AppEvent};
use std::default::Default;

pub type Handle<T> = Rc<RefCell<T>>;
type WeakHandle<T> = rc::Weak<RefCell<T>>;
type ActorPair = (WeakHandle<GameObject>, sync::Weak<Component>);

pub struct FPS {
    counter: u32,
    last: f64,
    pub fps: u32,
}

impl FPS {
    pub fn new() -> FPS {
        let fps = FPS {
            counter: 0,
            last: now(),
            fps: 0,
        };

        fps
    }

    pub fn step(&mut self) {
        self.counter += 1;
        let curr = now();
        if curr - self.last > 1.0 {
            self.last = curr;
            self.fps = self.counter;
            self.counter = 0;
        }
    }
}

pub struct World {
    engine: AppEngine,
    main_tree: Rc<SceneTree>,

    app: Option<App>,
    fps: FPS,

    actors: Vec<ActorPair>,
    new_actors: Rc<RefCell<Vec<ActorPair>>>,

    shown_stats: bool,
    events: Rc<RefCell<Vec<AppEvent>>>,

    golist: Vec<Handle<GameObject>>,
}

pub struct WorldBuilder<'a> {
    title: &'a str,
    size: Option<(u32, u32)>,
    shown_stats: Option<bool>,
}

impl<'a> WorldBuilder<'a> {
    pub fn new(title: &str) -> WorldBuilder {
        WorldBuilder {
            title: title,
            size: None,
            shown_stats: None,
        }
    }

    pub fn with_size(mut self, size: (u32, u32)) -> WorldBuilder<'a> {
        self.size = Some(size);
        self
    }

    pub fn with_stats(mut self, stats: bool) -> WorldBuilder<'a> {
        self.shown_stats = Some(stats);
        self
    }

    pub fn build(self) -> World {
        let size = self.size.unwrap_or((800, 600));
        let config = AppConfig::new(self.title, size);
        let app = App::new(config);

        let hidpi = app.hidpi_factor();
        let engine = Engine::new(
            app.canvas(),
            (
                ((size.0 as f32) * hidpi) as u32,
                ((size.1 as f32) * hidpi) as u32,
            ),
            hidpi,
        );
        let events = app.events.clone();
        let main_tree = engine.new_scene_tree();

        let mut w = World {
            engine,
            app: Some(app),
            main_tree,
            actors: Default::default(),
            new_actors: Default::default(),
            shown_stats: self.shown_stats.unwrap_or(false),
            fps: FPS::new(),
            events: events,
            golist: Vec::new(),
        };

        // Add a default camera
        w.engine.main_camera = Some(Rc::new(RefCell::new(Camera::new())));

        w.main_tree.add_watcher({
            let actors = w.new_actors.clone();

            move |changed, ref go, ref c: &Arc<Component>| {
                match changed {
                    ComponentEvent::Add => {
                        // filter
                        if c.try_as::<Box<Actor>>().is_some() {
                            actors
                                .borrow_mut()
                                .push((Rc::downgrade(go), Arc::downgrade(c)));
                        }
                    }

                    ComponentEvent::Remove => {
                        // filter
                        if c.try_as::<Box<Actor>>().is_some() {
                            actors.borrow_mut().retain(|&(_, ref cc)| {
                                let ccp = cc.upgrade();
                                if ccp.is_none() {
                                    return true;
                                }

                                !Arc::ptr_eq(ccp.as_ref().unwrap(), &c)
                            });
                        }
                    }
                }
            }
        });

        w
    }
}

impl World {
    pub fn root(&self) -> Ref<GameObject> {
        self.main_tree.root()
    }

    pub fn root_mut(&self) -> RefMut<GameObject> {
        self.main_tree.root_mut()
    }

    pub fn now() -> f64 {
        now()
    }

    pub fn engine(&self) -> &AppEngine {
        &self.engine
    }

    fn active_starting_actors(&mut self) {
        let mut starting = Vec::new();
        starting.append(&mut self.new_actors.borrow_mut());

        for &(ref wgo, ref c) in starting.iter() {
            let com = c.upgrade().unwrap().clone();
            let actor = com.try_as::<Box<Actor>>().unwrap();
            let go = wgo.upgrade().unwrap();

            (*actor).borrow_mut().start_rc(go, self);
        }

        self.actors.append(&mut starting);
    }

    fn step(&mut self) {
        for evt in self.events.borrow().iter() {
            match evt {
                &AppEvent::Resized(size) => self.engine.resize(size),
                _ => (),
            }
        }

        self.active_starting_actors();

        let mut actor_components = Vec::new();
        {
            let actors = &mut self.actors;

            for &(ref wgo, ref c) in actors.iter() {
                if let Some(go) = wgo.upgrade() {
                    actor_components.push((go.clone(), c.clone()));
                }
            }
        }

        for (go, c) in actor_components.into_iter() {
            let com = c.upgrade().unwrap().clone();
            let actor = com.try_as::<Box<Actor>>().unwrap();

            (*actor).borrow_mut().update_rc(go, self);
        }

        let actors = &mut self.actors;
        // move back and remove all unused components
        actors.retain(|&(_, ref c)| c.upgrade().is_some());

        use engine::imgui::Metric::*;

        self.fps.step();

        if self.shown_stats {
            imgui::pivot((0.0, 0.0));
            imgui::label(
                Native(0.0, 0.0) + Pixel(8.0, 8.0),
                &format!(
                    "fps: {} nobj: {} actors:{} lists:{}",
                    self.fps.fps,
                    self.engine().objects.len(),
                    self.actors.len(),
                    self.main_tree.len(),
                ),
            );
        }
    }

    pub fn events(&self) -> Ref<Vec<AppEvent>> {
        self.events.borrow()
    }

    pub fn asset_system<'a>(&'a self) -> &'a AssetSystem {
        self.engine.asset_system()
    }

    pub fn reset(&mut self) {
        self.actors.clear();
        self.golist.clear();
        self.engine.asset_system_mut().reset();
        self.main_tree.root_mut().clear_components();
    }

    pub fn event_loop(mut self) {
        let app = self.app.take().unwrap();

        app.run(move |_app: &mut App| {
            self.engine.begin();

            self.step();

            // Render
            self.engine.render(ClearOption::default());

            // End
            self.engine.end();
        });
    }

    pub fn new_game_object(&mut self) -> Handle<GameObject> {
        let go = self.engine.new_game_object(&self.main_tree.root());
        self.golist.push(go.clone());
        go
    }

    pub fn remove_game_object(&mut self, go: &Handle<GameObject>) {
        self.golist.retain(|ref x| !Rc::ptr_eq(&x, go));
    }
}

#[derive(Default)]
struct EmptyActor {}

impl Actor for EmptyActor {
    fn new() -> Box<Actor> {
        Box::new(EmptyActor::default())
    }
}

pub trait Actor {
    // Called before first update call
    fn start_rc(&mut self, go: Handle<GameObject>, world: &mut World) {
        self.start(&mut go.borrow_mut(), world)
    }

    // Called before first update call, with GameObject itself
    fn start(&mut self, &mut GameObject, &mut World) {}

    fn update_rc(&mut self, go: Handle<GameObject>, world: &mut World) {
        self.update(&mut go.borrow_mut(), world)
    }

    fn update(&mut self, &mut GameObject, &mut World) {}

    fn new() -> Box<Actor>
    where
        Self: Sized;

    fn new_actor<T>(a: T) -> Box<Actor>
    where
        Self: Sized,
        T: 'static + Actor,
    {
        Box::new(a)
    }
}

impl ComponentBased for Box<Actor> {}
