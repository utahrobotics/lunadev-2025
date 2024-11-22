extends Node2D

@onready var textbox=$CanvasLayer/VizHBox/DataVBox/Texbox/ScrollContainer/textVBox
@onready var map_texture=$"CanvasLayer/VizHBox/ImagePanel/VBoxContainer/MapTexture"
@onready var map_title= $CanvasLayer/VizHBox/ImagePanel/VBoxContainer/Title
var current_map := 0

var maps:Array[Image] = [null,null,null,null,null]

func set_image_maps(depth:Image,point:Image,height:Image,gradient:Image,obstacle:Image):
	maps[0]=depth
	maps[1]=depth
	maps[2]=depth
	maps[3]=depth
	maps[4]=depth
	
# Called when the node enters the scene tree for the first time.
func _ready() -> void:
	pass


# Called every frame. 'delta' is the elapsed time since the previous frame.
func _process(delta: float) -> void:
	map_texture.texture=maps[current_map]
	if map_texture.texture==null: generate_placeholder(64,64)

func set_message(msg: String, color: Color):
	var message:=Label.new()
	message.text=msg
	message.vertical_alignment=VERTICAL_ALIGNMENT_CENTER
	message.autowrap_mode=TextServer.AUTOWRAP_ARBITRARY
	message.custom_minimum_size=Vector2(151,20)
	message.add_theme_color_override("font_color",color)
	message.size_flags_vertical=Control.SIZE_SHRINK_BEGIN
	textbox.add_child(message)

func generate_placeholder(width:int, height:int):
	var image = Image.create(width, height, false, Image.FORMAT_RGBA8)
	for y in range(height):
		for x in range(width):
			var color = Color(x / float(width), y / float(height), 0.5, 1.0)
			image.set_pixel(x,y,color)
	var texture= ImageTexture.create_from_image(image)
	map_texture.texture=texture



func _on_depth_map_button_down() -> void:
	current_map=0
	map_title.text="Depth Map"
	set_message("Depth Map Selected",Color.WHITE)


func _on_point_map_button_down() -> void:
	current_map=1
	map_title.text="Point Map"
	set_message("Point Map Selected",Color.WHITE)


func _on_height_map_button_down() -> void:
	current_map=2
	map_title.text="Height Map"
	set_message("Height Map Selected",Color.WHITE)


func _on_gradient_map_button_down() -> void:
	current_map=3
	map_title.text="Gradient Map"
	set_message("Gradient Map Selected",Color.WHITE)

func _on_obstacle_map_button_down() -> void:
	current_map=4
	map_title.text="Obstacle Map"
	set_message("Obstacle Map Selected",Color.WHITE)


func _on_send_button_button_up() -> void:
	print(map_title.text)
	set_message(map_title.text+" Sent",Color.WHITE)
