use crate::libs::types::AdvancedFlowControl;

impl AdvancedFlowControl {
    // Define the bit positions
    const QFC_SHIFT: u32 = 31; // bit<1>
    const PIPE_ID_SHIFT: u32 = 29;
    const MAC_ID_SHIFT: u32 = 25;
    const QID_SHIFT: u32 = 15;
    const CREDIT_SHIFT: u32 = 0;

    // Define the bit masks
    const QFC_MASK: u32 = 0b1 << Self::QFC_SHIFT;
    const PIPE_ID_MASK: u32 = 0b11 << Self::PIPE_ID_SHIFT;
    const MAC_ID_MASK: u32 = 0b1111 << Self::MAC_ID_SHIFT;
    const QID_MASK: u32 = 0b1111111 << Self::QID_SHIFT;
    const CREDIT_MASK: u32 = 0b111111111111111 << Self::CREDIT_SHIFT;

    // Constructor
    pub fn new(pipe_id: u8, mac_id: u8, qid: u8, credit: u16) -> Self {
        let mut afc = AdvancedFlowControl { value: 0 };

        afc.set_qfc(1);
        afc.set_pipe_id(pipe_id.into());
        afc.set_mac_id(mac_id.into());
        afc.set_qid(qid.into());
        afc.set_credit(credit.into());

        afc
    }

    // Method to set bits
    fn set_bits(&mut self, pos: u32, mask: u32, value: u32) {
        self.value = (self.value & !mask) | ((value << pos) & mask);
    }

    // Method to get bits
    fn get_bits(&self, pos: u32, mask: u32) -> u32 {
        (self.value & mask) >> pos
    }

    // Setters
    fn set_qfc(&mut self, value: u32) {
        self.set_bits(Self::QFC_SHIFT, Self::QFC_MASK, value);
    }

    fn set_pipe_id(&mut self, value: u32) {
        self.set_bits(Self::PIPE_ID_SHIFT, Self::PIPE_ID_MASK, value);
    }

    fn set_mac_id(&mut self, value: u32) {
        self.set_bits(Self::MAC_ID_SHIFT, Self::MAC_ID_MASK, value);
    }

    fn set_qid(&mut self, value: u32) {
        self.set_bits(Self::QID_SHIFT, Self::QID_MASK, value);
    }

    fn set_credit(&mut self, value: u32) {
        self.set_bits(Self::CREDIT_SHIFT, Self::CREDIT_MASK, value);
    }

    // Getters
    fn _get_qfc(&self) -> u32 {
        self.get_bits(Self::QFC_SHIFT, Self::QFC_MASK)
    }

    fn _get_pipe_id(&self) -> u32 {
        self.get_bits(Self::PIPE_ID_SHIFT, Self::PIPE_ID_MASK)
    }

    fn _get_mac_id(&self) -> u32 {
        self.get_bits(Self::MAC_ID_SHIFT, Self::MAC_ID_MASK)
    }

    fn _get_qid(&self) -> u32 {
        self.get_bits(Self::QID_SHIFT, Self::QID_MASK)
    }

    fn _get_credit(&self) -> u32 {
        self.get_bits(Self::CREDIT_SHIFT, Self::CREDIT_MASK)
    }
}
