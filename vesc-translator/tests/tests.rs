use vesc_translator::*;

macro_rules! uart_binary_test {
    ($message:expr, $result:literal, $name:ident) => {
        #[test]
        fn $name() {
            let msg: Message = $message;
            assert_eq!(msg.to_uart_binary(), $result.to_be_bytes()[6..].to_vec());
        }
    }
}

#[test]
fn body_binary_test() {
    let msg = Message::new(CommandType::SetRpm, 0, 0x1234 as f32);
    assert_eq!(msg.to_body_binary(), 0x1234_u32.to_be_bytes().to_vec());
}

uart_binary_test!(Message::new(CommandType::SetRpm, 0, 1.0), 0x02050800000001120c03_u128, uart_binary_test_set_rpm_1);
uart_binary_test!(Message::new(CommandType::SetRpm, 1, 1.0), 0x02050800000001120c03_u128, uart_binary_test_set_rpm_2);
uart_binary_test!(Message::new_no_target(CommandType::SetRpm, 2.0), 0x02050800000002226f03_u128, uart_binary_test_set_rpm_3);
uart_binary_test!(Message::new(CommandType::SetDutyCycle, 0, 0.0), 0x02050500000000235703_u128, uart_binary_test_set_duty_cycle_1);
uart_binary_test!(Message::new(CommandType::SetDutyCycle, 2, 128E-5), 0x02050500000080b2df03_u128, uart_binary_test_set_duty_cycle_2);
uart_binary_test!(Message::new_no_target(CommandType::SetDutyCycle, -4E-5), 0x020505fffffffc8afb03_u128, uart_binary_test_set_duty_cycle_3);
