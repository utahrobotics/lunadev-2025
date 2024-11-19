extends Node2D

@onready var textbox=$CanvasLayer/VizHBox/DataVBox/Texbox/ScrollContainer/textVBox

# Called when the node enters the scene tree for the first time.
func _ready() -> void:
	set_message("Hello",Color.RED)


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
