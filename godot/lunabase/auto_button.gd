extends Button

func _ready() -> void:
	grab_focus()

func _on_pressed() -> void:
	$"../Buttons Label".text = "Current Stage: Auto"
	$"../../StateInfoTabContainer".current_tab=0
	#Lunabot.runautonomous()
