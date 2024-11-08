extends Path3D

@export var test_path:Array[Vector2]
@export var height = 0.25
var path_mat : Material=preload("res://path_material.tres")
@onready var robot = $"../Robot"
@onready var line_mesh = $CSGPolygon3D


# Called when the node enters the scene tree for the first time.
func _ready() -> void:
	line_mesh.material=path_mat
	create_path(test_path)

func create_path(path:Array[Vector2]):
	self.curve.add_point(Vector3(robot.position.x, height, robot.position.z))
	for i in path.size():
		self.curve.add_point(Vector3(path[i].x, height, path[i].y))
		place_marker(path[i].x,path[i].y,str(i))

#places cylinder maker for every point in path
# x and z parameters are for marker position
#num is for identifying which marker is being placed
func place_marker(x:float,z:float,num:String):
		var marker = CSGCylinder3D.new()
		marker.position=Vector3(x, height/2, z)
		marker.height=height
		marker.radius=0.05
		marker.name = str("marker",num)
		marker.material=path_mat
		add_child(marker)
