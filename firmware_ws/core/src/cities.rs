//! The built-in list of places for the setup page.
//!
//! Nobody knows their latitude and longitude, so the page lets the owner pick a place by name and
//! fills in the rest: the coordinates the prayer times are computed from, the standard UTC offset,
//! and the summer-time rule. The phone has no internet while it is on the lamp's own network, so
//! the list lives in the lamp's flash and is sent to the browser as plain text, which filters it.
//!
//! [`LIST`] is the one source of truth: one place per line, `Name|Country|lat|lon|utc|dst`, sorted
//! by name. The code here only reads it back, so the page and the tests see the same data.
//!
//! - `lat`, `lon`: decimal degrees with exactly two decimals, the block's own resolution.
//! - `utc`: the standard (winter) offset from UTC in minutes, for example `120` or `345`.
//! - `dst`: `none`, `egypt`, `europe` or `us`, the only summer-time rules the lamp can follow.
//! - `Name` is at most 15 bytes, because that is all the settings block stores of a place name.
//!   `Country` may carry a well-known other name in brackets, such as `Saudi Arabia (Mecca)`, so
//!   a search for either finds the place.
//!
//! What is left out on purpose: any place whose summer time the lamp cannot follow. That is
//! everywhere in Australia, New Zealand and Chile that moves its clocks in the southern summer,
//! and Israel, Palestine, Lebanon, Paraguay and Cuba, which have rules of their own. A place that
//! is right for half the year is worse than no place, and the page still takes coordinates by hand.
//! Morocco is listed at +60 all year; it steps back an hour for Ramadan, which the lamp cannot
//! follow either, so its owners adjust the offset in the menu for that month.

use crate::settings::Dst;

/// One place, read from a line of [`LIST`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct City {
    pub name: &'static str,
    pub country: &'static str,
    /// Latitude in hundredths of a degree: 30.04 is 3004.
    pub lat_e2: i16,
    /// Longitude in hundredths of a degree.
    pub lon_e2: i16,
    /// Standard offset from UTC in minutes.
    pub utc_min: i16,
    pub dst: Dst,
}

/// A decimal with exactly two decimals (`-0.13`, `30.04`) as hundredths (`-13`, `3004`).
fn hundredths(s: &str) -> Option<i16> {
    let (neg, s) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let (whole, frac) = s.split_once('.')?;
    if frac.len() != 2 || whole.is_empty() || whole.len() > 3 {
        return None;
    }
    if !whole.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v = whole.parse::<i16>().ok()? * 100 + frac.parse::<i16>().ok()?;
    Some(if neg { -v } else { v })
}

impl City {
    /// Read one `Name|Country|lat|lon|utc|dst` line, or `None` if it is not one.
    pub fn parse(line: &'static str) -> Option<City> {
        let mut f = line.split('|');
        let name = f.next().filter(|n| !n.is_empty())?;
        let country = f.next().filter(|c| !c.is_empty())?;
        let lat_e2 = hundredths(f.next()?)?;
        let lon_e2 = hundredths(f.next()?)?;
        let utc_min = f.next()?.parse().ok()?;
        let dst = Dst::from_name(f.next()?)?;
        if f.next().is_some() {
            return None;
        }
        Some(City { name, country, lat_e2, lon_e2, utc_min, dst })
    }
}

/// Every place in [`LIST`], in its order.
pub fn cities() -> impl Iterator<Item = City> {
    LIST.lines().filter_map(City::parse)
}

/// The places, one per line: `Name|Country|lat|lon|utc|dst`, sorted by name.
pub static LIST: &str = "\
10th of Ramadan|Egypt|30.29|31.75|120|egypt\n\
6th of October|Egypt|29.96|30.92|120|egypt\n\
Abbottabad|Pakistan|34.15|73.21|300|none\n\
Abha|Saudi Arabia|18.22|42.51|180|none\n\
Abidjan|Ivory Coast|5.36|-4.01|0|none\n\
Abu Dhabi|United Arab Emirates|24.45|54.38|240|none\n\
Abuja|Nigeria|9.08|7.40|60|none\n\
Accra|Ghana|5.60|-0.19|0|none\n\
Adana|Turkey|37.00|35.32|180|none\n\
Addis Ababa|Ethiopia|9.03|38.74|180|none\n\
Aden|Yemen|12.78|45.04|180|none\n\
Aga|Egypt|30.95|31.29|120|egypt\n\
Agadir|Morocco|30.43|-9.60|60|none\n\
Ahmedabad|India|23.02|72.57|330|none\n\
Ahvaz|Iran|31.32|48.67|210|none\n\
Aktau|Kazakhstan|43.65|51.15|300|none\n\
Al Ain|United Arab Emirates|24.21|55.76|240|none\n\
Al Khobar|Saudi Arabia|26.28|50.21|180|none\n\
Aleppo|Syria|36.20|37.16|180|none\n\
Alexandria|Egypt|31.20|29.92|120|egypt\n\
Algiers|Algeria|36.75|3.06|60|none\n\
Almaty|Kazakhstan|43.24|76.89|300|none\n\
Ambon|Indonesia|-3.70|128.18|540|none\n\
Amman|Jordan|31.95|35.93|180|none\n\
Amritsar|India|31.63|74.87|330|none\n\
Amsterdam|Netherlands|52.37|4.90|60|europe\n\
Anchorage|United States|61.22|-149.90|-540|us\n\
Ankara|Turkey|39.93|32.86|180|none\n\
Antalya|Turkey|36.90|30.70|180|none\n\
Antananarivo|Madagascar|-18.88|47.51|180|none\n\
Antwerp|Belgium|51.22|4.40|60|europe\n\
Aqaba|Jordan|29.53|35.01|180|none\n\
Ashgabat|Turkmenistan|37.96|58.33|300|none\n\
Asmara|Eritrea|15.34|38.93|180|none\n\
Astana|Kazakhstan|51.17|71.45|300|none\n\
Aswan|Egypt|24.09|32.90|120|egypt\n\
Asyut|Egypt|27.18|31.18|120|egypt\n\
Athens|Greece|37.98|23.73|120|europe\n\
Atlanta|United States|33.75|-84.39|-300|us\n\
Baghdad|Iraq|33.31|44.37|180|none\n\
Baku|Azerbaijan|40.41|49.87|240|none\n\
Balikpapan|Indonesia|-1.24|116.83|480|none\n\
Bamako|Mali|12.64|-8.00|0|none\n\
Banda Aceh|Indonesia|5.55|95.32|420|none\n\
Bandar Abbas|Iran|27.18|56.27|210|none\n\
Bandar Brunei|Brunei|4.89|114.94|480|none\n\
Bandar Lampung|Indonesia|-5.43|105.26|420|none\n\
Bandung|Indonesia|-6.92|107.61|420|none\n\
Bangkok|Thailand|13.76|100.50|420|none\n\
Banjarmasin|Indonesia|-3.32|114.59|480|none\n\
Banjul|Gambia|13.45|-16.58|0|none\n\
Barcelona|Spain|41.39|2.17|60|europe\n\
Barisal|Bangladesh|22.70|90.37|360|none\n\
Basra|Iraq|30.51|47.78|180|none\n\
Beijing|China|39.90|116.41|480|none\n\
Belbeis|Egypt|30.42|31.56|120|egypt\n\
Belfast|United Kingdom|54.60|-5.93|0|europe\n\
Belgrade|Serbia|44.79|20.45|60|europe\n\
Bengaluru|India (Bangalore)|12.97|77.59|330|none\n\
Benghazi|Libya|32.12|20.07|120|none\n\
Benha|Egypt|30.46|31.18|120|egypt\n\
Beni Suef|Egypt|29.07|31.10|120|egypt\n\
Berlin|Germany|52.52|13.40|60|europe\n\
Bern|Switzerland|46.95|7.45|60|europe\n\
Bhopal|India|23.26|77.41|330|none\n\
Bhubaneswar|India|20.30|85.82|330|none\n\
Bilqas|Egypt|31.23|31.37|120|egypt\n\
Birmingham|United Kingdom|52.49|-1.89|0|europe\n\
Bishkek|Kyrgyzstan|42.87|74.59|360|none\n\
Bogota|Colombia|4.71|-74.07|-300|none\n\
Boston|United States|42.36|-71.06|-300|us\n\
Brasilia|Brazil|-15.79|-47.88|-180|none\n\
Bratislava|Slovakia|48.15|17.11|60|europe\n\
Brisbane|Australia|-27.47|153.03|600|none\n\
Brussels|Belgium|50.85|4.35|60|europe\n\
Bucharest|Romania|44.43|26.10|120|europe\n\
Budapest|Hungary|47.50|19.04|60|europe\n\
Buenos Aires|Argentina|-34.60|-58.38|-180|none\n\
Bukhara|Uzbekistan|39.77|64.42|300|none\n\
Buraidah|Saudi Arabia|26.33|43.97|180|none\n\
Bursa|Turkey|40.19|29.06|180|none\n\
Busan|South Korea|35.18|129.08|540|none\n\
Cairo|Egypt|30.04|31.24|120|egypt\n\
Calgary|Canada|51.05|-114.07|-420|us\n\
Cancun|Mexico|21.16|-86.85|-300|none\n\
Cape Town|South Africa|-33.92|18.42|120|none\n\
Caracas|Venezuela|10.48|-66.90|-240|none\n\
Cardiff|United Kingdom|51.48|-3.18|0|europe\n\
Casablanca|Morocco|33.57|-7.59|60|none\n\
Cebu|Philippines|10.32|123.89|480|none\n\
Chengdu|China|30.57|104.07|480|none\n\
Chennai|India (Madras)|13.08|80.27|330|none\n\
Chiang Mai|Thailand|18.79|98.98|420|none\n\
Chicago|United States|41.88|-87.63|-360|us\n\
Chittagong|Bangladesh|22.36|91.78|360|none\n\
Cologne|Germany|50.94|6.96|60|europe\n\
Colombo|Sri Lanka|6.93|79.86|330|none\n\
Comilla|Bangladesh|23.46|91.18|360|none\n\
Conakry|Guinea|9.64|-13.58|0|none\n\
Constantine|Algeria|36.37|6.61|60|none\n\
Copenhagen|Denmark|55.68|12.57|60|europe\n\
Cordoba|Argentina|-31.42|-64.18|-180|none\n\
Da Nang|Vietnam|16.05|108.20|420|none\n\
Dahab|Egypt|28.50|34.51|120|egypt\n\
Dakar|Senegal|14.72|-17.47|0|none\n\
Dallas|United States|32.78|-96.80|-360|us\n\
Damanhur|Egypt|31.03|30.47|120|egypt\n\
Damascus|Syria|33.51|36.29|180|none\n\
Damietta|Egypt|31.42|31.81|120|egypt\n\
Dammam|Saudi Arabia|26.43|50.10|180|none\n\
Dar es Salaam|Tanzania|-6.79|39.21|180|none\n\
Darwin|Australia|-12.46|130.84|570|none\n\
Davao|Philippines|7.07|125.61|480|none\n\
Dearborn|United States|42.32|-83.18|-300|us\n\
Deir ez-Zor|Syria|35.34|40.14|180|none\n\
Denpasar|Indonesia|-8.65|115.22|480|none\n\
Denver|United States|39.74|-104.99|-420|us\n\
Desouk|Egypt|31.13|30.65|120|egypt\n\
Detroit|United States|42.33|-83.05|-300|us\n\
Dhahran|Saudi Arabia|26.29|50.11|180|none\n\
Dhaka|Bangladesh|23.81|90.41|360|none\n\
Dikirnis|Egypt|31.09|31.59|120|egypt\n\
Diyarbakir|Turkey|37.91|40.24|180|none\n\
Djibouti|Djibouti|11.59|43.15|180|none\n\
Dodoma|Tanzania|-6.16|35.75|180|none\n\
Doha|Qatar|25.29|51.53|180|none\n\
Douala|Cameroon|4.05|9.77|60|none\n\
Dubai|United Arab Emirates|25.20|55.27|240|none\n\
Dublin|Ireland|53.35|-6.26|0|europe\n\
Durban|South Africa|-29.86|31.03|120|none\n\
Dushanbe|Tajikistan|38.56|68.77|300|none\n\
Edfu|Egypt|24.98|32.87|120|egypt\n\
Edinburgh|United Kingdom|55.95|-3.19|0|europe\n\
Edmonton|Canada|53.55|-113.49|-420|us\n\
El Arish|Egypt|31.13|33.80|120|egypt\n\
El Kharga|Egypt|25.45|30.55|120|egypt\n\
El Mahalla|Egypt|30.97|31.17|120|egypt\n\
Erbil|Iraq|36.19|44.01|180|none\n\
Erzurum|Turkey|39.90|41.27|180|none\n\
Faisalabad|Pakistan|31.42|73.08|300|none\n\
Faiyum|Egypt|29.31|30.84|120|egypt\n\
Fes|Morocco|34.03|-5.00|60|none\n\
Fortaleza|Brazil|-3.73|-38.53|-180|none\n\
Frankfurt|Germany|50.11|8.68|60|europe\n\
Freetown|Sierra Leone|8.48|-13.23|0|none\n\
Fujairah|United Arab Emirates|25.12|56.33|240|none\n\
Gaborone|Botswana|-24.65|25.91|120|none\n\
Gaziantep|Turkey|37.07|37.38|180|none\n\
Geneva|Switzerland|46.20|6.14|60|europe\n\
George Town|Malaysia|5.41|100.33|480|none\n\
Georgetown|Guyana|6.80|-58.16|-240|none\n\
Giza|Egypt|30.01|31.21|120|egypt\n\
Glasgow|United Kingdom|55.86|-4.25|0|europe\n\
Grozny|Russia|43.32|45.69|180|none\n\
Guadalajara|Mexico|20.67|-103.35|-360|none\n\
Guangzhou|China|23.13|113.26|480|none\n\
Guatemala City|Guatemala|14.63|-90.51|-360|none\n\
Guwahati|India|26.14|91.74|330|none\n\
Hail|Saudi Arabia|27.52|41.70|180|none\n\
Halifax|Canada|44.65|-63.57|-240|us\n\
Hama|Syria|35.13|36.75|180|none\n\
Hamburg|Germany|53.55|9.99|60|europe\n\
Hangzhou|China|30.27|120.15|480|none\n\
Hanoi|Vietnam|21.03|105.85|420|none\n\
Harare|Zimbabwe|-17.83|31.05|120|none\n\
Hargeisa|Somalia|9.56|44.06|180|none\n\
Hat Yai|Thailand|7.01|100.47|420|none\n\
Helsinki|Finland|60.17|24.94|120|europe\n\
Herat|Afghanistan|34.35|62.20|270|none\n\
Ho Chi Minh|Vietnam (Saigon)|10.82|106.63|420|none\n\
Hodeidah|Yemen|14.80|42.95|180|none\n\
Hofuf|Saudi Arabia|25.38|49.59|180|none\n\
Homs|Syria|34.73|36.72|180|none\n\
Hong Kong|Hong Kong|22.32|114.17|480|none\n\
Honolulu|United States|21.31|-157.86|-600|none\n\
Houston|United States|29.76|-95.37|-360|us\n\
Hurghada|Egypt|27.26|33.81|120|egypt\n\
Hyderabad|India|17.39|78.49|330|none\n\
Hyderabad|Pakistan|25.39|68.37|300|none\n\
Irbid|Jordan|32.56|35.85|180|none\n\
Isfahan|Iran|32.65|51.67|210|none\n\
Islamabad|Pakistan|33.68|73.05|300|none\n\
Ismailia|Egypt|30.59|32.27|120|egypt\n\
Istanbul|Turkey|41.01|28.98|180|none\n\
Izmir|Turkey|38.42|27.14|180|none\n\
Jaipur|India|26.91|75.79|330|none\n\
Jakarta|Indonesia|-6.18|106.85|420|none\n\
Jayapura|Indonesia|-2.53|140.72|540|none\n\
Jeddah|Saudi Arabia|21.54|39.17|180|none\n\
Jizan|Saudi Arabia|16.89|42.55|180|none\n\
Johannesburg|South Africa|-26.20|28.05|120|none\n\
Johor Bahru|Malaysia|1.49|103.74|480|none\n\
Juba|South Sudan|4.85|31.58|120|none\n\
Kabul|Afghanistan|34.53|69.17|270|none\n\
Kafr El Dawar|Egypt|31.13|30.13|120|egypt\n\
Kafr El Sheikh|Egypt|31.11|30.94|120|egypt\n\
Kafr El Zayat|Egypt|30.82|30.82|120|egypt\n\
Kampala|Uganda|0.35|32.58|180|none\n\
Kandahar|Afghanistan|31.63|65.74|270|none\n\
Kano|Nigeria|12.00|8.52|60|none\n\
Karachi|Pakistan|24.86|67.01|300|none\n\
Karbala|Iraq|32.62|44.02|180|none\n\
Kathmandu|Nepal|27.72|85.32|345|none\n\
Kayseri|Turkey|38.73|35.49|180|none\n\
Kazan|Russia|55.80|49.11|180|none\n\
Kermanshah|Iran|34.31|47.07|210|none\n\
Khamis Mushait|Saudi Arabia|18.31|42.73|180|none\n\
Khartoum|Sudan|15.50|32.56|120|none\n\
Khulna|Bangladesh|22.85|89.54|360|none\n\
Kigali|Rwanda|-1.95|30.06|120|none\n\
Kingston|Jamaica|17.97|-76.79|-300|none\n\
Kinshasa|DR Congo|-4.44|15.27|60|none\n\
Kirkuk|Iraq|35.47|44.39|180|none\n\
Kochi|India|9.93|76.27|330|none\n\
Kolkata|India (Calcutta)|22.57|88.36|330|none\n\
Kom Ombo|Egypt|24.47|32.95|120|egypt\n\
Konya|Turkey|37.87|32.48|180|none\n\
Kota Bharu|Malaysia|6.13|102.24|480|none\n\
Kota Kinabalu|Malaysia|5.98|116.07|480|none\n\
Kozhikode|India|11.26|75.78|330|none\n\
Kuala Lumpur|Malaysia|3.14|101.69|480|none\n\
Kuwait City|Kuwait|29.38|47.99|180|none\n\
Kyiv|Ukraine|50.45|30.52|120|europe\n\
La Paz|Bolivia|-16.50|-68.15|-240|none\n\
Lagos|Nigeria|6.52|3.38|60|none\n\
Lahore|Pakistan|31.55|74.34|300|none\n\
Latakia|Syria|35.52|35.79|180|none\n\
Leeds|United Kingdom|53.80|-1.55|0|europe\n\
Libreville|Gabon|0.42|9.47|60|none\n\
Lima|Peru|-12.05|-77.04|-300|none\n\
Lisbon|Portugal|38.72|-9.14|0|europe\n\
Liverpool|United Kingdom|53.41|-2.99|0|europe\n\
Ljubljana|Slovenia|46.06|14.51|60|europe\n\
London|United Kingdom|51.51|-0.13|0|europe\n\
Los Angeles|United States|34.05|-118.24|-480|us\n\
Luanda|Angola|-8.84|13.23|60|none\n\
Lucknow|India|26.85|80.95|330|none\n\
Lusaka|Zambia|-15.39|28.32|120|none\n\
Luxembourg|Luxembourg|49.61|6.13|60|europe\n\
Luxor|Egypt|25.70|32.64|120|egypt\n\
Lyon|France|45.76|4.84|60|europe\n\
Madaba|Jordan|31.72|35.79|180|none\n\
Madinah|Saudi Arabia (Medina)|24.47|39.61|180|none\n\
Madrid|Spain|40.42|-3.70|60|europe\n\
Makassar|Indonesia|-5.15|119.43|480|none\n\
Makkah|Saudi Arabia (Mecca)|21.39|39.86|180|none\n\
Malaga|Spain|36.72|-4.42|60|europe\n\
Manado|Indonesia|1.49|124.84|480|none\n\
Manama|Bahrain|26.23|50.59|180|none\n\
Manaus|Brazil|-3.12|-60.02|-240|none\n\
Manchester|United Kingdom|53.48|-2.24|0|europe\n\
Mandalay|Myanmar|21.98|96.08|390|none\n\
Manila|Philippines|14.60|120.98|480|none\n\
Mansoura|Egypt|31.04|31.38|120|egypt\n\
Maputo|Mozambique|-25.97|32.57|120|none\n\
Marrakesh|Morocco|31.63|-8.01|60|none\n\
Marsa Matruh|Egypt|31.35|27.24|120|egypt\n\
Mashhad|Iran|36.30|59.60|210|none\n\
Mazar-i-Sharif|Afghanistan|36.71|67.11|270|none\n\
Medan|Indonesia|3.60|98.67|420|none\n\
Melaka|Malaysia|2.19|102.25|480|none\n\
Mersin|Turkey|36.80|34.64|180|none\n\
Mexico City|Mexico|19.43|-99.13|-360|none\n\
Miami|United States|25.76|-80.19|-300|us\n\
Milan|Italy|45.46|9.19|60|europe\n\
Minneapolis|United States|44.98|-93.27|-360|us\n\
Minsk|Belarus|53.90|27.57|180|none\n\
Minya|Egypt|28.11|30.75|120|egypt\n\
Misrata|Libya|32.38|15.09|120|none\n\
Mit Ghamr|Egypt|30.72|31.26|120|egypt\n\
Mogadishu|Somalia|2.05|45.32|180|none\n\
Mombasa|Kenya|-4.04|39.67|180|none\n\
Monterrey|Mexico|25.67|-100.31|-360|none\n\
Montevideo|Uruguay|-34.90|-56.16|-180|none\n\
Montreal|Canada|45.50|-73.57|-300|us\n\
Moscow|Russia|55.76|37.62|180|none\n\
Mosul|Iraq|36.34|43.13|180|none\n\
Multan|Pakistan|30.20|71.47|300|none\n\
Mumbai|India (Bombay)|19.08|72.88|330|none\n\
Munich|Germany|48.14|11.58|60|europe\n\
Muscat|Oman|23.59|58.41|240|none\n\
Nagoya|Japan|35.18|136.91|540|none\n\
Nagpur|India|21.15|79.09|330|none\n\
Nairobi|Kenya|-1.29|36.82|180|none\n\
Najaf|Iraq|32.00|44.34|180|none\n\
Najran|Saudi Arabia|17.49|44.13|180|none\n\
Nanjing|China|32.06|118.80|480|none\n\
Naples|Italy|40.85|14.27|60|europe\n\
Nasr City|Egypt|30.06|31.34|120|egypt\n\
Ndjamena|Chad|12.13|15.06|60|none\n\
New Cairo|Egypt|30.03|31.47|120|egypt\n\
New Delhi|India|28.61|77.21|330|none\n\
New Orleans|United States|29.95|-90.07|-360|us\n\
New York|United States|40.71|-74.01|-300|us\n\
Niamey|Niger|13.51|2.11|60|none\n\
Nicosia|Cyprus|35.19|33.38|120|europe\n\
Nizwa|Oman|22.93|57.53|240|none\n\
Nouakchott|Mauritania|18.09|-15.98|0|none\n\
Novosibirsk|Russia|55.03|82.92|420|none\n\
Omdurman|Sudan|15.64|32.48|120|none\n\
Oran|Algeria|35.70|-0.63|60|none\n\
Osaka|Japan|34.69|135.50|540|none\n\
Oslo|Norway|59.91|10.75|60|europe\n\
Ottawa|Canada|45.42|-75.70|-300|us\n\
Ouagadougou|Burkina Faso|12.37|-1.52|0|none\n\
Padang|Indonesia|-0.95|100.35|420|none\n\
Palembang|Indonesia|-2.98|104.76|420|none\n\
Panama City|Panama|8.98|-79.52|-300|none\n\
Paramaribo|Suriname|5.85|-55.20|-180|none\n\
Paris|France|48.86|2.35|60|europe\n\
Patna|India|25.59|85.14|330|none\n\
Pattani|Thailand|6.87|101.25|420|none\n\
Pekanbaru|Indonesia|0.51|101.45|420|none\n\
Perth|Australia|-31.95|115.86|480|none\n\
Peshawar|Pakistan|34.01|71.58|300|none\n\
Philadelphia|United States|39.95|-75.17|-300|us\n\
Phnom Penh|Cambodia|11.56|104.93|420|none\n\
Phoenix|United States|33.45|-112.07|-420|none\n\
Phuket|Thailand|7.88|98.39|420|none\n\
Podgorica|Montenegro|42.44|19.26|60|europe\n\
Pontianak|Indonesia|-0.03|109.33|420|none\n\
Port of Spain|Trinidad|10.66|-61.51|-240|none\n\
Port Said|Egypt|31.27|32.30|120|egypt\n\
Port Sudan|Sudan|19.62|37.22|120|none\n\
Prague|Czechia|50.08|14.44|60|europe\n\
Prayagraj|India|25.44|81.85|330|none\n\
Pretoria|South Africa|-25.75|28.19|120|none\n\
Pristina|Kosovo|42.67|21.17|60|europe\n\
Pune|India|18.52|73.86|330|none\n\
Qatif|Saudi Arabia|26.56|50.01|180|none\n\
Qena|Egypt|26.16|32.72|120|egypt\n\
Qom|Iran|34.64|50.88|210|none\n\
Quebec City|Canada|46.81|-71.21|-300|us\n\
Quetta|Pakistan|30.18|66.98|300|none\n\
Quito|Ecuador|-0.18|-78.47|-300|none\n\
Quseir|Egypt|26.10|34.28|120|egypt\n\
Rabat|Morocco|34.02|-6.84|60|none\n\
Rajshahi|Bangladesh|24.37|88.60|360|none\n\
Rangpur|Bangladesh|25.74|89.25|360|none\n\
Ras Al Khaimah|United Arab Emirates|25.79|55.94|240|none\n\
Rasht|Iran|37.28|49.58|210|none\n\
Rawalpindi|Pakistan|33.60|73.04|300|none\n\
Regina|Canada|50.45|-104.62|-360|none\n\
Reykjavik|Iceland|64.15|-21.94|0|none\n\
Riga|Latvia|56.95|24.11|120|europe\n\
Rio de Janeiro|Brazil|-22.91|-43.17|-180|none\n\
Riyadh|Saudi Arabia|24.71|46.68|180|none\n\
Rome|Italy|41.90|12.50|60|europe\n\
Rotterdam|Netherlands|51.92|4.48|60|europe\n\
Safaga|Egypt|26.74|33.94|120|egypt\n\
Salalah|Oman|17.02|54.09|240|none\n\
Salt Lake City|United States|40.76|-111.89|-420|us\n\
Salvador|Brazil|-12.97|-38.51|-180|none\n\
Samarkand|Uzbekistan|39.65|66.96|300|none\n\
Samsun|Turkey|41.29|36.33|180|none\n\
San Diego|United States|32.72|-117.16|-480|us\n\
San Francisco|United States|37.77|-122.42|-480|us\n\
San Jose|Costa Rica|9.93|-84.08|-360|none\n\
Sanaa|Yemen|15.35|44.21|180|none\n\
Sanliurfa|Turkey|37.16|38.80|180|none\n\
Santo Domingo|Dominican Rep|18.49|-69.93|-240|none\n\
Sao Paulo|Brazil|-23.55|-46.63|-180|none\n\
Sapporo|Japan|43.06|141.35|540|none\n\
Sarajevo|Bosnia|43.86|18.41|60|europe\n\
Saskatoon|Canada|52.13|-106.67|-360|none\n\
Seattle|United States|47.61|-122.33|-480|us\n\
Semarang|Indonesia|-6.97|110.42|420|none\n\
Seoul|South Korea|37.57|126.98|540|none\n\
Seville|Spain|37.39|-5.98|60|europe\n\
Sfax|Tunisia|34.74|10.76|60|none\n\
Shanghai|China|31.23|121.47|480|none\n\
Sharm El Sheikh|Egypt|27.91|34.33|120|egypt\n\
Shenzhen|China|22.54|114.06|480|none\n\
Shibin El Kom|Egypt|30.56|31.01|120|egypt\n\
Shiraz|Iran|29.59|52.58|210|none\n\
Shymkent|Kazakhstan|42.32|69.59|300|none\n\
Sinbillawin|Egypt|30.88|31.48|120|egypt\n\
Singapore|Singapore|1.35|103.82|480|none\n\
Siwa|Egypt|29.20|25.52|120|egypt\n\
Skopje|North Macedonia|41.99|21.43|60|europe\n\
Sofia|Bulgaria|42.70|23.32|120|europe\n\
Sohag|Egypt|26.56|31.69|120|egypt\n\
Sohar|Oman|24.35|56.71|240|none\n\
Srinagar|India|34.08|74.80|330|none\n\
St Johns|Canada|47.56|-52.71|-210|us\n\
St Petersburg|Russia|59.93|30.34|180|none\n\
Stockholm|Sweden|59.33|18.07|60|europe\n\
Suez|Egypt|29.97|32.55|120|egypt\n\
Sulaymaniyah|Iraq|35.56|45.43|180|none\n\
Surabaya|Indonesia|-7.25|112.75|420|none\n\
Sylhet|Bangladesh|24.90|91.87|360|none\n\
Tabriz|Iran|38.08|46.29|210|none\n\
Tabuk|Saudi Arabia|28.38|36.57|180|none\n\
Taif|Saudi Arabia|21.27|40.42|180|none\n\
Taipei|Taiwan|25.03|121.57|480|none\n\
Taiz|Yemen|13.58|44.02|180|none\n\
Talkha|Egypt|31.05|31.37|120|egypt\n\
Tallinn|Estonia|59.44|24.75|120|europe\n\
Tangier|Morocco|35.76|-5.80|60|none\n\
Tanta|Egypt|30.79|31.00|120|egypt\n\
Tashkent|Uzbekistan|41.30|69.24|300|none\n\
Tbilisi|Georgia|41.72|44.79|240|none\n\
Tehran|Iran|35.69|51.39|210|none\n\
Thessaloniki|Greece|40.64|22.94|120|europe\n\
Tirana|Albania|41.33|19.82|60|europe\n\
Tokyo|Japan|35.68|139.69|540|none\n\
Toronto|Canada|43.65|-79.38|-300|us\n\
Trabzon|Turkey|41.00|39.72|180|none\n\
Tripoli|Libya|32.89|13.19|120|none\n\
Trivandrum|India|8.52|76.94|330|none\n\
Tunis|Tunisia|36.81|10.18|60|none\n\
Ufa|Russia|54.74|55.97|300|none\n\
Ulaanbaatar|Mongolia|47.89|106.91|480|none\n\
Urumqi|China|43.83|87.62|480|none\n\
Valletta|Malta|35.90|14.51|60|europe\n\
Vancouver|Canada|49.28|-123.12|-480|us\n\
Varanasi|India|25.32|82.97|330|none\n\
Vienna|Austria|48.21|16.37|60|europe\n\
Vientiane|Laos|17.97|102.60|420|none\n\
Vilnius|Lithuania|54.69|25.28|120|europe\n\
Visakhapatnam|India|17.69|83.22|330|none\n\
Vladivostok|Russia|43.12|131.89|600|none\n\
Warsaw|Poland|52.23|21.01|60|europe\n\
Washington DC|United States|38.91|-77.04|-300|us\n\
Winnipeg|Canada|49.90|-97.14|-360|us\n\
Wuhan|China|30.59|114.31|480|none\n\
Xian|China|34.34|108.94|480|none\n\
Yanbu|Saudi Arabia|24.09|38.06|180|none\n\
Yangon|Myanmar (Rangoon)|16.87|96.20|390|none\n\
Yaounde|Cameroon|3.85|11.50|60|none\n\
Yazd|Iran|31.90|54.37|210|none\n\
Yekaterinburg|Russia|56.84|60.61|300|none\n\
Yerevan|Armenia|40.18|44.51|240|none\n\
Yogyakarta|Indonesia|-7.80|110.36|420|none\n\
Zagazig|Egypt|30.59|31.50|120|egypt\n\
Zagreb|Croatia|45.81|15.98|60|europe\n\
Zamboanga|Philippines|6.91|122.07|480|none\n\
Zarqa|Jordan|32.07|36.09|180|none\n\
Zifta|Egypt|30.71|31.24|120|egypt\n\
Zurich|Switzerland|47.38|8.54|60|europe\n\
";

#[cfg(test)]
mod tests {
    use super::*;
    use core::cmp::Ordering;

    fn cmp_ci(a: &str, b: &str) -> Ordering {
        a.bytes().map(|c| c.to_ascii_lowercase()).cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
    }

    fn find(name: &str, country_prefix: &str) -> City {
        cities()
            .find(|c| c.name == name && c.country.starts_with(country_prefix))
            .unwrap_or_else(|| panic!("{name} ({country_prefix}) is not in the list"))
    }

    #[test]
    fn every_line_parses_and_the_list_ends_with_a_newline() {
        assert!(LIST.ends_with('\n'));
        assert!(!LIST.contains("\n\n"));
        for line in LIST.lines() {
            assert!(City::parse(line).is_some(), "does not parse: {line}");
        }
        assert_eq!(cities().count(), LIST.lines().count());
    }

    #[test]
    fn the_list_is_a_sensible_size() {
        let n = cities().count();
        assert!((250..=500).contains(&n), "{n} places");
        assert!(LIST.len() < 26_000, "{} bytes", LIST.len());
        assert!(LIST.is_ascii());
    }

    #[test]
    fn names_fit_the_settings_block() {
        for c in cities() {
            assert!(c.name.len() <= 15, "{} is {} bytes", c.name, c.name.len());
            assert!(!c.name.contains('|') && !c.country.contains('|'));
            assert!(c.name.bytes().all(|b| (0x20..0x7F).contains(&b)), "{}", c.name);
            assert!(c.country.bytes().all(|b| (0x20..0x7F).contains(&b)), "{}", c.country);
        }
    }

    #[test]
    fn numbers_are_in_range() {
        for c in cities() {
            assert!((-9000..=9000).contains(&c.lat_e2), "{}", c.name);
            assert!((-18000..=18000).contains(&c.lon_e2), "{}", c.name);
            assert_eq!(c.utc_min % 15, 0, "{}", c.name);
            assert!((-720..=840).contains(&c.utc_min), "{}", c.name);
        }
    }

    #[test]
    fn the_list_is_sorted_by_name_then_country_with_no_duplicates() {
        let mut prev: Option<City> = None;
        for c in cities() {
            if let Some(p) = prev {
                let order = cmp_ci(p.name, c.name).then(cmp_ci(p.country, c.country));
                assert_eq!(order, Ordering::Less, "{} ({}) after {} ({})", c.name, c.country, p.name, p.country);
            }
            prev = Some(c);
        }
    }

    #[test]
    fn summer_time_rules_are_only_where_they_apply() {
        for c in cities() {
            match c.dst {
                Dst::Egypt => assert!(c.country == "Egypt" && c.utc_min == 120, "{}", c.name),
                Dst::Us => assert!(
                    c.country == "United States" || c.country == "Canada",
                    "{} is not in the US or Canada",
                    c.name
                ),
                Dst::Europe => {
                    assert!([0, 60, 120].contains(&c.utc_min), "{} at {}", c.name, c.utc_min)
                }
                Dst::None => {}
            }
        }
    }

    #[test]
    fn the_offset_roughly_matches_the_longitude() {
        // A zone is about 15 degrees wide, and politics stretches it. Forty degrees either way
        // catches a sign slip or a wrong digit without arguing about where a border runs. There
        // are no exceptions: places that would need one (far-west China, for example) are left out.
        for c in cities() {
            let zone_centre_e2 = c.utc_min as i32 * 25; // minutes / 60 * 15 degrees, in hundredths
            let off = (zone_centre_e2 - c.lon_e2 as i32).abs();
            assert!(off <= 4000, "{}: utc {} but longitude {}", c.name, c.utc_min, c.lon_e2);
        }
    }

    #[test]
    fn the_hemispheres_and_signs_are_right_for_well_known_places() {
        assert!(cities().filter(|c| c.country == "Egypt").all(|c| c.lat_e2 > 2000 && c.lat_e2 < 3200));
        assert!(cities().filter(|c| c.country == "Egypt").all(|c| c.lon_e2 > 2400 && c.lon_e2 < 3600));
        for c in cities().filter(|c| matches!(c.country, "United States" | "Canada" | "Brazil" | "Argentina")) {
            assert!(c.lon_e2 < 0, "{} should be west of Greenwich", c.name);
        }
        assert!(find("Johannesburg", "South Africa").lat_e2 < 0);
        assert!(find("Sao Paulo", "Brazil").lat_e2 < 0);
        assert!(find("Brisbane", "Australia").lat_e2 < 0);
        assert!(find("Jakarta", "Indonesia").lat_e2 < 0);
        assert!(find("Medan", "Indonesia").lat_e2 > 0);
    }

    #[test]
    fn spot_checks_of_well_known_places() {
        let cairo = find("Cairo", "Egypt");
        assert_eq!((cairo.lat_e2, cairo.lon_e2, cairo.utc_min, cairo.dst), (3004, 3124, 120, Dst::Egypt));
        let makkah = find("Makkah", "Saudi Arabia");
        assert_eq!((makkah.lat_e2, makkah.lon_e2, makkah.utc_min, makkah.dst), (2139, 3986, 180, Dst::None));
        let london = find("London", "United Kingdom");
        assert_eq!((london.lat_e2, london.lon_e2, london.utc_min, london.dst), (5151, -13, 0, Dst::Europe));
        let paris = find("Paris", "France");
        assert_eq!((paris.utc_min, paris.dst), (60, Dst::Europe));
        let ny = find("New York", "United States");
        assert_eq!((ny.lat_e2, ny.lon_e2, ny.utc_min, ny.dst), (4071, -7401, -300, Dst::Us));
        let la = find("Los Angeles", "United States");
        assert_eq!((la.utc_min, la.dst), (-480, Dst::Us));
        let phoenix = find("Phoenix", "United States");
        assert_eq!((phoenix.utc_min, phoenix.dst), (-420, Dst::None));
        let kolkata = find("Kolkata", "India");
        assert_eq!((kolkata.utc_min, kolkata.dst), (330, Dst::None));
        assert_eq!(find("Kathmandu", "Nepal").utc_min, 345);
        assert_eq!(find("Jakarta", "Indonesia").utc_min, 420);
        assert_eq!(find("Makassar", "Indonesia").utc_min, 480);
        assert_eq!(find("Jayapura", "Indonesia").utc_min, 540);
        let istanbul = find("Istanbul", "Turkey");
        assert_eq!((istanbul.utc_min, istanbul.dst), (180, Dst::None));
        let casablanca = find("Casablanca", "Morocco");
        assert_eq!((casablanca.utc_min, casablanca.dst), (60, Dst::None));
        assert_eq!(find("Tehran", "Iran").utc_min, 210);
        assert_eq!(find("Kabul", "Afghanistan").utc_min, 270);
        assert_eq!(find("Dubai", "United Arab Emirates").utc_min, 240);
        assert_eq!(find("Karachi", "Pakistan").utc_min, 300);
        assert_eq!(find("Dhaka", "Bangladesh").utc_min, 360);
        assert_eq!(find("Kuala Lumpur", "Malaysia").utc_min, 480);
        assert_eq!(find("Darwin", "Australia").utc_min, 570);
        assert_eq!(find("Toronto", "Canada").dst, Dst::Us);
        assert_eq!(find("Regina", "Canada").dst, Dst::None);
        assert_eq!(find("St Johns", "Canada").utc_min, -210);
    }

    #[test]
    fn the_nile_delta_is_covered() {
        for name in [
            "Mansoura", "Mit Ghamr", "Zagazig", "Tanta", "Damanhur", "Kafr El Sheikh", "Damietta",
            "Shibin El Kom", "Benha", "Aga", "Talkha",
        ] {
            let c = find(name, "Egypt");
            assert!((3000..=3200).contains(&c.lat_e2) && (3000..=3200).contains(&c.lon_e2), "{name}");
        }
    }

    #[test]
    fn places_that_the_lamp_cannot_follow_are_absent() {
        for name in ["Sydney", "Melbourne", "Auckland", "Wellington", "Santiago", "Asuncion", "Havana", "Jerusalem", "Tel Aviv", "Beirut", "Gaza"] {
            assert!(cities().all(|c| c.name != name), "{name} should not be listed");
        }
    }

    #[test]
    fn a_line_is_read_back_exactly() {
        let c = City::parse("Quito|Ecuador|-0.18|-78.47|-300|none").unwrap();
        assert_eq!((c.name, c.country, c.lat_e2, c.lon_e2, c.utc_min, c.dst), ("Quito", "Ecuador", -18, -7847, -300, Dst::None));
        // What is not a line is not read as one.
        assert_eq!(City::parse(""), None);
        assert_eq!(City::parse("Quito|Ecuador|-0.18|-78.47|-300"), None);
        assert_eq!(City::parse("Quito|Ecuador|-0.18|-78.47|-300|none|extra"), None);
        assert_eq!(City::parse("Quito|Ecuador|-0.1|-78.47|-300|none"), None);
        assert_eq!(City::parse("Quito|Ecuador|-0.180|-78.47|-300|none"), None);
        assert_eq!(City::parse("Quito|Ecuador|x.18|-78.47|-300|none"), None);
        assert_eq!(City::parse("Quito|Ecuador|-0.18|-78.47|-300|mars"), None);
        assert_eq!(City::parse("|Ecuador|-0.18|-78.47|-300|none"), None);
    }
}
