control TimestampRegister(
            in bit<48> new_timestamp,
            out bit<48> last_timestamp) {

    Register<bit<32>, bit<32>>(4096, 0) lower_last_ts_series;
    Register<bit<16>, bit<32>>(4096, 0) higher_last_ts_series;

    // Write lower 32 bit of timestamp 
    RegisterAction<bit<32>, bit<32>, bit<32>>(lower_last_ts_series) read_and_set_lower_last_ts = {
            void apply(inout bit<32> value, out bit<32> read_value) {
                read_value = value;
                value = new_timestamp[31:0];
            }
    };    

    // Write higher 16 bit of timestamp 
    RegisterAction<bit<16>, bit<32>, bit<16>>(higher_last_ts_series) read_and_set_higher_last_ts = {
            void apply(inout bit<16> value, out bit<16> read_value) {
                read_value = value;
                value = new_timestamp[47:32];
            }
    };


    apply {
        last_timestamp[31:0] = read_and_set_lower_last_ts.execute(0);
        last_timestamp[47:32] = read_and_set_higher_last_ts.execute(0);
    }
}