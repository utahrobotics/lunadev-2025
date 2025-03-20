use std::{collections::{vec_deque, VecDeque}, time::{Duration, Instant}};

use common::{FromLunabase, PathPoint};
use nalgebra::{distance, Isometry3, Point2, Point3};
use simple_motion::StaticImmutableNode;

use crate::{autonomy::Autonomy, Action, PollWhen};

pub enum Input {
    FromLunabase(FromLunabase),
    PathCalculated(Vec<PathPoint>),
    FailedToCalculatePath(Vec<PathPoint>),
    LunabaseDisconnected,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PathfindingState {
    Idle,
    Pending,
    Failed,
}

pub(crate) struct LunabotBlackboard {
    now: Instant,
    from_lunabase: VecDeque<FromLunabase>,
    autonomy: Autonomy,
    chain: StaticImmutableNode,
    path: Vec<PathPoint>,
    pathfinding_state: PathfindingState,
    lunabase_disconnected: bool,
    actions: Vec<Action>,
    poll_when: PollWhen,
    
    backtracking: bool,
    
    /// stack of completed MoveTo points within 0.5 meters from latest position 
    recently_completed_pts: Vec<Point3<f64>>,
    
    /// (position, timestamp)
    latest_position: Option<(Point3<f64>, Instant)>,
}

impl LunabotBlackboard {
    pub fn new(chain: StaticImmutableNode) -> Self {
        Self {
            now: Instant::now(),
            from_lunabase: Default::default(),
            autonomy: Autonomy::None,
            path: vec![],
            pathfinding_state: PathfindingState::Idle,
            chain,
            lunabase_disconnected: true,
            actions: vec![],
            poll_when: PollWhen::NoDelay,
            
            backtracking: false,
            recently_completed_pts: vec![].into(),
            latest_position: None
        }
    }
}

impl LunabotBlackboard {
    pub fn peek_from_lunabase(&self) -> Option<&FromLunabase> {
        self.from_lunabase.front()
    }

    pub fn pop_from_lunabase(&mut self) -> Option<FromLunabase> {
        self.from_lunabase.pop_front()
    }

    pub fn get_autonomy(&mut self) -> &mut Autonomy {
        &mut self.autonomy
    }

    pub fn get_poll_when(&mut self) -> &mut PollWhen {
        &mut self.poll_when
    }

    pub fn get_robot_isometry(&self) -> Isometry3<f64> {
        self.chain.get_global_isometry()
    }

    pub fn get_path(&self) -> Option<&[PathPoint]> {
        if self.path.is_empty() {
            None
        } else {
            Some(&self.path)
        }
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
    
    pub fn backtracking(&mut self) -> &mut bool {
        &mut self.backtracking
    }
    
    pub fn get_latest_position(&self) -> Option<(Point3<f64>, Instant)> {
        self.latest_position
    }
    
    pub fn set_latest_position(&mut self, pos: Point3<f64>) {
        self.latest_position = Some((pos, self.now));
    }
    
    pub fn pop_completed_pt(&mut self) -> Option<Point3<f64>> {
        self.recently_completed_pts.pop()
    }
    
    pub fn push_completed_pt(&mut self, pos: Point3<f64>) {
        self.recently_completed_pts.push(pos);
    }
    
    pub fn clear_completed_pts(&mut self) {
        self.recently_completed_pts.clear();
    }

    pub fn digest_input(&mut self, input: Input) {
        match input {
            Input::FromLunabase(msg) => self.from_lunabase.push_back(msg),
            Input::PathCalculated(path) => {
                self.path = path;
                self.pathfinding_state = PathfindingState::Idle;
            }
            Input::FailedToCalculatePath(path) => {
                self.path = path;
                self.pathfinding_state = PathfindingState::Failed;
            }
            Input::LunabaseDisconnected => self.lunabase_disconnected = true,
        }
    }

    pub fn calculate_path(&mut self, from: Point3<f64>, to: Point3<f64>) {
        let into = std::mem::take(&mut self.path);
        self.pathfinding_state = PathfindingState::Pending;
        
        
        self.enqueue_action(Action::CalculatePath { from, to, into });
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
