use std::{cell::Cell, collections::VecDeque, time::Instant};

use common::{world_point_to_cell, CellsRect, FromLunabase, PathPoint, PathKind};
use nalgebra::{Isometry3, Point3, UnitQuaternion};
use simple_motion::StaticImmutableNode;

use crate::{autonomy::AutonomyState, Action, PollWhen};

pub enum Input {
    FromLunabase(FromLunabase),
    LunabaseDisconnected,
    
    PathCalculated(Vec<PathPoint>),
    FailedToCalculatePath,
    PathDestIsKnown,
    
    DoneExploring,
    NotDoneExploring((usize, usize)),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PathfindingState {
    Idle,
    Pending,
    Failed,
    PathDestIsKnown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CheckIfExploredState {
    HaveToCheck,
    Pending,
    NeedToExplore((usize, usize)),
    FinishedExploring
}

pub(crate) struct LunabotBlackboard {
    now: Instant,
    from_lunabase: VecDeque<FromLunabase>,
    chain: StaticImmutableNode,
    path: Option<Vec<PathPoint>>,
    lunabase_disconnected: bool,
    actions: Vec<Action>,
    poll_when: PollWhen,
    
    autonomy_state: AutonomyState,
    pathfinding_state: PathfindingState,
    check_if_explored_state: CheckIfExploredState,
    
    /// (position, rotation, timestamp)
    latest_transform: Option<(Point3<f64>, UnitQuaternion<f64>, Instant)>,
    backing_away_from: Option<Point3<f64>>,
    
}

impl LunabotBlackboard {
    pub fn new(chain: StaticImmutableNode) -> Self {
        Self {
            now: Instant::now(),
            from_lunabase: VecDeque::new(),
            chain,
            path: None,
            lunabase_disconnected: true,
            actions: vec![],
            poll_when: PollWhen::NoDelay,
            
            autonomy_state: AutonomyState::Start,
            pathfinding_state: PathfindingState::Idle,
            check_if_explored_state: CheckIfExploredState::HaveToCheck,
            
            backing_away_from: None,
            latest_transform: None,
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

    pub fn get_autonomy(&self) -> AutonomyState {
        self.autonomy_state
    }
    pub fn set_autonomy(&mut self, state: AutonomyState) {
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

    pub fn get_path(&self) -> &Option<Vec<PathPoint>> {
        &self.path
    }

    pub fn get_path_mut(&mut self) -> &mut Option<Vec<PathPoint>> {
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
                self.path = Some(path);
                self.pathfinding_state = PathfindingState::Idle;
            }
            Input::FailedToCalculatePath => {
                self.path = None;
                self.pathfinding_state = PathfindingState::Failed;
            }
            Input::PathDestIsKnown => {
                self.path = None;
                self.pathfinding_state = PathfindingState::PathDestIsKnown;
            }
            Input::LunabaseDisconnected => self.lunabase_disconnected = true,
            Input::DoneExploring => {
                self.check_if_explored_state = CheckIfExploredState::FinishedExploring;
            },
            Input::NotDoneExploring(cell) => {
                self.check_if_explored_state = CheckIfExploredState::NeedToExplore(cell);
            },
        }
    }
    
    pub fn get_target_cell(&self) -> Option<(usize, usize)> {
        match self.get_autonomy() {
            AutonomyState::Explore(cell) => Some(cell),
            AutonomyState::MoveToDumpSite(cell) => Some(cell),
            AutonomyState::MoveToDigSite(cell) => Some(cell),
            _ => None,
        }
    }

    pub fn request_for_path(&mut self, from: (usize, usize), to: (usize, usize), kind: PathKind, fail_if_dest_is_known: bool) {
        self.pathfinding_state = PathfindingState::Pending;
        self.enqueue_action(Action::CalculatePath { from, to, kind, fail_if_dest_is_known });
    }

    pub fn pathfinding_state(&self) -> PathfindingState {
        self.pathfinding_state
    }
    
    pub fn check_if_explored(&mut self, area: CellsRect) {
        self.check_if_explored_state = CheckIfExploredState::Pending;
        self.enqueue_action(Action::CheckIfExplored {
            area,
            robot_cell_pos: world_point_to_cell(self.get_robot_pos()),
        });
    }
    
    pub fn exploring_state(&self) -> CheckIfExploredState {
        self.check_if_explored_state
    }
    
    pub fn enqueue_action(&mut self, action: Action) {
        self.actions.push(action);
    }

    pub fn drain_actions(&mut self) -> std::vec::Drain<Action> {
        self.actions.drain(..)
    }
}
