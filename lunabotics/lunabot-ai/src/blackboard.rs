use std::{collections::VecDeque, time::Instant};

use common::{world_point_to_cell, FromLunabase, PathPoint, PathKind};
use nalgebra::{Isometry3, Point3, UnitQuaternion, Vector2, Vector3};
use simple_motion::StaticImmutableNode;

use crate::{autonomy::AutonomyState, Action, PollWhen};

pub enum Input {
    FromLunabase(FromLunabase),
    LunabaseDisconnected,
    
    PathCalculated(Vec<PathPoint>),
    FailedToCalculatePath,
    
    NextActionSite((usize, usize)),
    NoActionSite,

    ActuatorReadings {
        lift: u16,
        tilt: u16
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PathfindingState {
    Idle,
    Pending,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FindActionSiteState {
    Start,
    #[allow(dead_code)]
    Pending,
    FoundSite((usize, usize)),
    NotFound
}



pub(crate) struct LunabotBlackboard {
    now: Instant,
    from_lunabase: VecDeque<FromLunabase>,
    chain: StaticImmutableNode,
    path: Vec<PathPoint>,
    lunabase_disconnected: bool,
    actions: Vec<Action>,
    poll_when: PollWhen,
    
    autonomy_state: AutonomyState,
    pathfinding_state: PathfindingState,
    find_action_site_state: FindActionSiteState,
    
    /// (position, rotation, timestamp)
    latest_transform: Option<(Point3<f64>, UnitQuaternion<f64>, Instant)>,
    backing_away_from: Option<Point3<f64>>,

    actual_lift: u16,
    actual_tilt: u16,

    lift_travel_positive: bool,
    tilt_travel_positive: bool,

    target_lift: u16,
    target_tilt: u16
    
}

impl LunabotBlackboard {
    pub fn new(chain: StaticImmutableNode) -> Self {
        Self {
            now: Instant::now(),
            from_lunabase: VecDeque::new(),
            chain,
            path: vec![],
            lunabase_disconnected: true,
            actions: vec![],
            poll_when: PollWhen::NoDelay,
            
            autonomy_state: AutonomyState::None,
            pathfinding_state: PathfindingState::Idle,
            find_action_site_state: FindActionSiteState::Start,
            
            backing_away_from: None,
            latest_transform: None,

            actual_lift: 0,
            actual_tilt: 0,

            lift_travel_positive: false,
            tilt_travel_positive: false,

            target_lift: 0,
            target_tilt: 0,
        }
    }
}

impl LunabotBlackboard {
    pub fn get_actual_lift(&self) -> u16 {
        self.actual_lift
    }
    pub fn get_actual_tilt(&self) -> u16 {
        self.actual_tilt
    }
    pub fn get_target_lift(&mut self) -> &mut u16 {
        &mut self.target_lift
    }
    pub fn get_target_tilt(&mut self) -> &mut u16 {
        &mut self.target_tilt
    }
    pub fn get_lift_travel_positive(&mut self) -> &mut bool {
        &mut self.lift_travel_positive
    }
    pub fn get_tilt_travel_positive(&mut self) -> &mut bool {
        &mut self.tilt_travel_positive
    }
    pub fn peek_from_lunabase(&self) -> Option<&FromLunabase> {
        self.from_lunabase.front()
    }

    pub fn pop_from_lunabase(&mut self) -> Option<FromLunabase> {
        self.from_lunabase.pop_front()
    }

    pub fn get_autonomy_state(&self) -> AutonomyState {
        self.autonomy_state
    }
    pub fn set_autonomy_state(&mut self, state: AutonomyState) {
        self.autonomy_state = state;
    }

    pub fn get_poll_when(&mut self) -> &mut PollWhen {
        &mut self.poll_when
    }

    pub fn get_robot_isometry(&self) -> Isometry3<f64> {
        self.chain.get_global_isometry()
    }
    pub fn get_robot_pos(&self) -> Point3<f64> {
        self.get_robot_isometry().translation.vector.into()
    }
    /// returns a unit vector of the direction the robot is facing
    pub fn get_robot_heading(&self) -> Vector2<f64> {
        self.get_robot_isometry()
            .rotation
            .transform_vector(&Vector3::new(0.0, 0.0, -1.0))
            .xz()
    }

    pub fn get_path(&self) -> &Vec<PathPoint> {
        &self.path
    }

    pub fn get_path_mut(&mut self) -> &mut Vec<PathPoint> {
        &mut self.path
    }
    
    pub fn lunabase_disconnected(&mut self) -> &mut bool {
        &mut self.lunabase_disconnected
    }

    pub fn get_now(&self) -> Instant {
        self.now
    }

    pub(crate) fn update_now(&mut self) {
        self.now = Instant::now();
    }
    
    pub fn backing_away_from(&mut self) -> &mut Option<Point3<f64>> {
        &mut self.backing_away_from
    }
    
    pub fn get_latest_transform(&self) -> Option<(Point3<f64>, UnitQuaternion<f64>, Instant)> {
        self.latest_transform
    }
    
    pub fn set_latest_transform(&mut self, pos: Point3<f64>, heading: UnitQuaternion<f64>) {
        self.latest_transform = Some((pos, heading, self.now));
    }
    
    pub fn clear_latest_transform(&mut self) {
        self.latest_transform = None;
    }
    
    pub fn digest_input(&mut self, input: Input) {
        match input {
            Input::FromLunabase(msg) => self.from_lunabase.push_back(msg),
            Input::PathCalculated(path) => {
                self.path = path;
                self.pathfinding_state = PathfindingState::Idle;
            }
            Input::FailedToCalculatePath => {
                self.path = vec![];
                self.pathfinding_state = PathfindingState::Failed;
            }
            Input::LunabaseDisconnected => self.lunabase_disconnected = true,
            Input::NextActionSite(cell) => {
                self.find_action_site_state = FindActionSiteState::FoundSite(cell);
            }
            Input::NoActionSite => {
                self.find_action_site_state = FindActionSiteState::NotFound;
            }
            Input::ActuatorReadings { lift, tilt } => {
                self.actual_lift = lift;
                self.actual_tilt = tilt;
            }
        }
    }
    
    pub fn get_target_cell(&self) -> Option<(usize, usize)> {
        
        // TODO set hardcoded traverse/dump positions
        match self.get_autonomy_state() {
            AutonomyState::ToExcavationZone(p) => Some(world_point_to_cell(Point3::new(p.x, 0.0, p.y))),
            AutonomyState::Dump(p) => Some(world_point_to_cell(Point3::new(p.x, 0.0, p.y))),
            AutonomyState::None => None,
        }
    }

    pub fn request_for_path(&mut self, from: (usize, usize), to: (usize, usize), kind: PathKind) {
        self.pathfinding_state = PathfindingState::Pending;
        self.enqueue_action(Action::CalculatePath { from, to, kind });
    }

    pub fn pathfinding_state(&self) -> PathfindingState {
        self.pathfinding_state
    }
    
    pub fn enqueue_action(&mut self, action: Action) {
        self.actions.push(action);
    }

    pub fn drain_actions(&mut self) -> std::vec::Drain<Action> {
        self.actions.drain(..)
    }
}
