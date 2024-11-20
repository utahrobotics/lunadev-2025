extends Node2D

@onready var textbox=$CanvasLayer/VizHBox/DataVBox/Texbox/ScrollContainer/textVBox
@onready var map_texture=$"CanvasLayer/VizHBox/ImagePanel/VBoxContainer/MapTexture"
@onready var map_title= $CanvasLayer/VizHBox/ImagePanel/VBoxContainer/Title
var current_map = "Depth"
# Called when the node enters the scene tree for the first time.
func _ready() -> void:
	map_title.text=current_map+" Map"
	generate_image(64,64)


# Called every frame. 'delta' is the elapsed time since the previous frame.
func _process(delta: float) -> void:
	pass

func set_message(msg: String, color: Color):
	var message:=Label.new()
	message.text=msg
	message.vertical_alignment=VERTICAL_ALIGNMENT_CENTER
	message.autowrap_mode=TextServer.AUTOWRAP_ARBITRARY
	message.custom_minimum_size=Vector2(151,20)
	message.add_theme_color_override("font_color",color)
	message.size_flags_vertical=Control.SIZE_SHRINK_BEGIN
	textbox.add_child(message)

func generate_image(width:int, height:int):
	var image = Image.create(width, height, false, Image.FORMAT_RGBA8)
	for y in range(height):
		for x in range(width):
			var color = Color(x / float(width), y / float(height), 0.5, 1.0)
			image.set_pixel(x,y,color)
	var texture= ImageTexture.create_from_image(image)
	map_texture.texture=texture
	map_texture.set_size(Vector2(64, 64)) 
	print(map_texture.texture)



func _on_depth_map_button_down() -> void:
	current_map="Depth"
	map_title.text=current_map+" Map"
	set_message("Depth Map Selected",Color.WHITE)


func _on_point_map_button_down() -> void:
	current_map="Point"
	map_title.text=current_map+" Map"
	set_message("Point Map Selected",Color.WHITE)


func _on_height_map_button_down() -> void:
	current_map="Height"
	map_title.text=current_map+" Map"
	set_message("Height Map Selected",Color.WHITE)


func _on_gradient_map_button_down() -> void:
	current_map="Gradient"
	map_title.text=current_map+" Map"
	set_message("Gradient Map Selected",Color.WHITE)

func _on_obstacle_map_button_down() -> void:
	current_map="Obstacle"
	map_title.text=current_map+" Map"
	set_message("Obstacle Map Selected",Color.WHITE)


func _on_send_button_button_up() -> void:
	print(current_map)
	set_message(current_map+" Map Sent",Color.WHITE)
