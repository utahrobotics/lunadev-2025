extends Button


func _on_pressed() -> void:
	Lunabot.current_state=Lunabot.State.AUTO
	$"../Buttons Label".text = "Current Stage: TeleOp"
	$"../../StateInfoTabContainer".current_tab=0
	#Lunabot.run_auto()
