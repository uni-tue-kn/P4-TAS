control TimestampSeriesRegister(
            in bit<48> new_timestamp,
            in bit<32> time_series_index) {

    Register<bit<32>, bit<32>>(4096, 0) lower_last_ts_series;
    Register<bit<16>, bit<32>>(4096, 0) higher_last_ts_series;

    // Write lower 32 bit of timestamp 
    RegisterAction<bit<32>, bit<32>, void>(lower_last_ts_series) set_lower_last_ts = {
            void apply(inout bit<32> value) {
                value = new_timestamp[31:0];
            }
    };    

    // Write higher 16 bit of timestamp 
    RegisterAction<bit<16>, bit<32>, void>(higher_last_ts_series) set_higher_last_ts = {
            void apply(inout bit<16> value) {
                value = new_timestamp[47:32];
            }
    };


    apply {
        set_lower_last_ts.execute(time_series_index);
        set_higher_last_ts.execute(time_series_index);
    }
}