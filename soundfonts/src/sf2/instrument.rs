use super::zone::Sf2Zone;
use soundfont::Instrument;

#[derive(Clone, Debug)]
pub struct Sf2Instrument {
    pub regions: Vec<Sf2Zone>,
}

impl Sf2Instrument {
    pub fn parse_instruments(instruments: Vec<Instrument>) -> Vec<Self> {
        let mut out: Vec<Self> = Vec::new();

        for instrument in instruments {
            let regions = Sf2Zone::parse(instrument.zones);

            out.push(Sf2Instrument { regions });
        }
        out
    }
}
