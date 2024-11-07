extends Button

func _ready() -> void:
	grab_focus()

func _on_pressed() -> void:
	Lunabot.current_state=Lunabot.State.AUTO
	$"../Buttons Label".text = "Current Stage: Auto"
	$"../../StateInfoTabContainer".current_tab=0
	#Lunabot.runautonomous()
