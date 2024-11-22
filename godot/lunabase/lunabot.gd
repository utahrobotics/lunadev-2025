extends LunabotConn

enum State { AUTO, TELEOP, STOPPED }

var current_state: State = State.STOPPED
