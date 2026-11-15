#!/usr/bin/env bash
# Retrace the worked example: build the glasshouse dictionary from what a
# reader types out of GZC-ICD-001 revision B, by the documented steps, and
# compare the result with dictionaries/glasshouse byte for byte. Then take it
# to revision C.
#
#   docs/icd-to-dictionary/06-worked-example/retrace.sh
#   BITE=target/release/bite docs/icd-to-dictionary/06-worked-example/retrace.sh
#   docs/icd-to-dictionary/06-worked-example/retrace.sh --update
#
# --update replaces dictionaries/glasshouse with the result instead of
# comparing, for when the document or the tables have been changed on purpose.
#
# Everything the script feeds in comes from the document: the four tables in
# tables/ (typed from sections 4 to 6), the dictionary.json written below, and
# the expected values and payloads of appendix B on the `bite vectors new`
# lines. retrace.out is the output of one run.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
if [ -z "${BITE:-}" ]; then
  (cd "$root" && cargo build --quiet --locked -p bite)
  BITE="$root/target/debug/bite"
fi
case "$BITE" in /*) ;; *) BITE="$PWD/$BITE" ;; esac

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work"
# Work on copies, so that every path printed is relative.
cp -R "$here/tables" tables
cp -R "$here/revision-c/tables" tables-c
[ "${1:-}" = "--update" ] || cp -R "$root/dictionaries/glasshouse" committed

# Print a command, then run it. `bite` stands for the binary's full path.
show() { printf '\n$'; for arg in "$@"; do case "$arg" in "$BITE") printf ' bite' ;; *' '*) printf ' "%s"' "$arg" ;; *) printf ' %s' "$arg" ;; esac; done; printf '\n'; }
run() { show "$@"; "$@"; }
# The same, for a command that is expected to fail.
fails() {
  show "$@"
  local status=0
  "$@" || status=$?
  if [ "$status" -eq 0 ]; then echo "retrace: expected that to fail" >&2; exit 1; fi
  echo "(exit status $status)"
}
step() { printf '\n=== %s ===\n' "$1"; }

step "1. Scaffold"
run "$BITE" new glasshouse --name glasshouse --interface ZONE --port 47010 --interface SUPV --port 47011

step "2. The numbering of section 3.1, checked on three fields"
run "$BITE" range --word-bits 16 --bit0 lsb --first-word 1 --word 1 --bits 15-8
run "$BITE" range --word-bits 16 --bit0 lsb --first-word 1 --word 6 --bits 15-6
run "$BITE" range --word-bits 16 --bit0 lsb --first-word 1 --word 7-8 --bits 15-8

step "3. dictionary.json: name, version, source document, added units, allowed findings"
cat > glasshouse/dictionary.json <<'JSON'
{
    "name": "glasshouse",
    "version": "1.0.0",
    "description": "Synthetic example: a glasshouse zone controller and its supervisor, transcribed from the invented document GZC-ICD-001 in docs/icd-to-dictionary/06-worked-example.",
    "source": {
        "document": "GZC-ICD-001",
        "revision": "B"
    },
    "lint": {
        "units": [
            "W/m2",
            "mS/cm"
        ],
        "allow": [
            {
                "rule": "C004",
                "at": "ZONE/Maintenance Data",
                "reason": "Defined in GZC-MNT-004, which is not transcribed here."
            }
        ]
    }
}
JSON
echo "wrote glasshouse/dictionary.json"

step "4. The tables exactly as the document prints them: Table 5-3 has an erratum"
mkdir as-printed
cp tables/*.csv as-printed/
sed -i.bak 's/^ZONE,Climate Report,leaf_wetness,6,5-0,\(.*\),"Table 5-3 prints.*$/ZONE,Climate Report,leaf_wetness,6,6-0,\1,/' as-printed/fields.csv
rm as-printed/fields.csv.bak
run "$BITE" import-csv as-printed --into glasshouse
fails "$BITE" check --strict glasshouse
fails "$BITE" coverage glasshouse

step "5. The tables with ISSUE-001 decided"
run "$BITE" import-csv tables --into glasshouse
run "$BITE" fmt glasshouse
run "$BITE" check --strict glasshouse
run "$BITE" lint glasshouse
fails "$BITE" coverage --strict glasshouse

step "6. One vector per message, from appendix B"
run "$BITE" vectors new glasshouse --interface ZONE --message "Zone Status" \
    --payload 01080c1102932e002500096f4e332020 \
    --note "GZC-ICD-001 revision B, appendix B.1." \
    --set message_id=1 --set message_length=8 --set zone_number=3 --set sequence_number=17 \
    --set time_of_day=43200000 --set operating_mode=Automatic --set door_open=false \
    --set heating_active=true --set irrigation_active=false --set alarm_present=true \
    --set supply_voltage=24.15 --set zone_label=N3
run "$BITE" vectors new glasshouse --interface ZONE --message "Climate Report" \
    --payload 020a0c12029331e8ff06cb6a76d0019f0dea01a9 \
    --note "GZC-ICD-001 revision B, appendix B.2." \
    --set message_id=2 --set message_length=10 --set zone_number=3 --set sequence_number=18 \
    --set time_of_day=43201000 --set air_temperature=-2.50 --set relative_humidity=81.3 \
    --set leaf_wetness=42 --set outside_temperature=-7.35 --set co2_concentration=415 \
    --set solar_irradiance=356.2 --set wind_speed=4.25
run "$BITE" vectors new glasshouse --interface ZONE --message "Irrigation Report" \
    --payload 03091fff014998f40520003212d6873e073a \
    --note "GZC-ICD-001 revision B, appendix B.3." \
    --set message_id=3 --set message_length=9 --set zone_number=7 --set sequence_number=1023 \
    --set time_of_day=21600500 --set valve_4_open=false --set valve_3_open=true \
    --set valve_2_open=false --set valve_1_open=true --set pump_state=Running \
    --set flow_rate=12.50 --set volume_delivered=123456.7 --set nutrient_solution_ph=6.2 \
    --set electrical_conductivity=1.850
run "$BITE" vectors new glasshouse --interface ZONE --message "Vent Report" \
    --payload 04070c13029335d09f404b001352 \
    --note "GZC-ICD-001 revision B, appendix B.4." \
    --set message_id=4 --set message_length=7 --set zone_number=3 --set sequence_number=19 \
    --set time_of_day=43202000 --set roof_vent_offset_sign=Negative \
    --set roof_vent_offset_magnitude=12.5 --set windward_vent_position=37.5 \
    --set leeward_vent_position=0.0 --set vent_drive_state=Opening --set motor_current=850
run "$BITE" vectors new glasshouse --interface ZONE --message "Alarm" \
    --payload 050730040038cefc058000030fa0 \
    --note "GZC-ICD-001 revision B, appendix B.5." \
    --set message_id=5 --set message_length=7 --set zone_number=12 --set sequence_number=4 \
    --set time_of_day=3723004 --set "alarm_code=Pump Fault" --set severity=Warning \
    --set acknowledged=false --set occurrence_count=3 --set minutes_since_raised=125
run "$BITE" vectors new glasshouse --interface ZONE --message "Event Log" \
    --payload 06090c14029339b802001102123412021240 \
    --note "GZC-ICD-001 revision B, appendix B.6." \
    --set message_id=6 --set message_length=9 --set zone_number=3 --set sequence_number=20 \
    --set time_of_day=43203000 --set record_count=2
run "$BITE" vectors new glasshouse --interface SUPV --message "Setpoint Command" \
    --payload 01080c050149970008ca0640af000320 \
    --note "GZC-ICD-001 revision B, appendix B.7." \
    --set message_id=1 --set message_length=8 --set zone_number=3 --set sequence_number=5 \
    --set time_of_day=21600000 --set day_temperature_setpoint=22.50 \
    --set night_temperature_setpoint=16.00 --set humidity_setpoint=70.0 --set co2_setpoint=800
run "$BITE" vectors new glasshouse --interface SUPV --message "Irrigation Command" \
    --payload 02071c06014997fa0500025809c4 \
    --note "GZC-ICD-001 revision B, appendix B.8." \
    --set message_id=2 --set message_length=7 --set zone_number=7 --set sequence_number=6 \
    --set time_of_day=21600250 --set open_valve_4=false --set open_valve_3=true \
    --set open_valve_2=false --set open_valve_1=true --set run_time=600 --set volume_limit=250.0
run "$BITE" vectors new glasshouse --interface SUPV --message "Vent Command" \
    --payload 03060c0702932a1864191000 \
    --note "GZC-ICD-001 revision B, appendix B.9." \
    --set message_id=3 --set message_length=6 --set zone_number=3 --set sequence_number=7 \
    --set time_of_day=43199000 --set windward_vent_target=50.0 --set leeward_vent_target=12.5 \
    --set "vent_command_mode=Go To Target"
run "$BITE" vectors new glasshouse --interface SUPV --message "Time Synchronisation" \
    --payload 04070c0800000000356ffe200000 \
    --note "GZC-ICD-001 revision B, appendix B.10." \
    --set message_id=4 --set message_length=7 --set zone_number=3 --set sequence_number=8 \
    --set time_of_day=0 --set year=26 --set month=11 --set day=15 --set utc_offset=-480 \
    --set daylight_saving_active=false

for vector in glasshouse/vectors/*.json; do
  name="$(basename "$vector" .json)"
  run "$BITE" vectors encode glasshouse --vector "$name"
done

step "7. The gates"
run "$BITE" fmt --check glasshouse
run "$BITE" check --strict glasshouse
run "$BITE" lint glasshouse
run "$BITE" coverage --strict glasshouse

step "8. Byte for byte against dictionaries/glasshouse (copied to committed/)"
if [ "${1:-}" = "--update" ]; then
  rm -rf "$root/dictionaries/glasshouse"
  cp -R glasshouse "$root/dictionaries/glasshouse"
  echo "replaced dictionaries/glasshouse"
else
  run diff -r glasshouse committed
  echo "identical"
fi

step "9. The tables round trip"
run "$BITE" export-csv glasshouse --out exported
mkdir again
cp glasshouse/dictionary.json glasshouse/README.md again/
cp -R glasshouse/vectors again/
run "$BITE" import-csv exported --into again
run diff -r glasshouse again
echo "identical"

step "10. Revision C of the document arrives"
cp -R glasshouse glasshouse-c
run "$BITE" import-csv tables-c --into glasshouse-c
sed -i.bak -e 's/"revision": "B"/"revision": "C"/' glasshouse-c/dictionary.json
rm glasshouse-c/dictionary.json.bak
fails "$BITE" check --strict glasshouse-c
fails "$BITE" diff --check-version glasshouse glasshouse-c

step "11. Vectors of the changed messages, from revision C's appendix B; version 2.0.0"
rm glasshouse-c/vectors/climate-report.json glasshouse-c/vectors/irrigation-report.json glasshouse-c/vectors/vent-report.json
run "$BITE" vectors new glasshouse-c --interface ZONE --message "Climate Report" \
    --payload 020b0c12029331e8ff06cb6a76d0019f0dea01a9fde8 \
    --note "GZC-ICD-001 revision C, appendix B.2." \
    --set message_id=2 --set message_length=11 --set zone_number=3 --set sequence_number=18 \
    --set time_of_day=43201000 --set air_temperature=-2.50 --set relative_humidity=81.3 \
    --set leaf_wetness=42 --set outside_temperature=-7.35 --set co2_concentration=415 \
    --set solar_irradiance=356.2 --set wind_speed=4.25 --set dew_point=-5.36
run "$BITE" vectors new glasshouse-c --interface ZONE --message "Irrigation Report" \
    --payload 03091fff014998f40520007d12d6873e073a \
    --note "GZC-ICD-001 revision C, appendix B.3." \
    --set message_id=3 --set message_length=9 --set zone_number=7 --set sequence_number=1023 \
    --set time_of_day=21600500 --set valve_4_open=false --set valve_3_open=true \
    --set valve_2_open=false --set valve_1_open=true --set pump_state=Running \
    --set flow_rate=12.5 --set volume_delivered=123456.7 --set nutrient_solution_ph=6.2 \
    --set electrical_conductivity=1.850
run "$BITE" vectors new glasshouse-c --interface ZONE --message "Vent Report" \
    --payload 04070c13029335d09f404b001352 \
    --note "GZC-ICD-001 revision C, appendix B.4." \
    --set message_id=4 --set message_length=7 --set zone_number=3 --set sequence_number=19 \
    --set time_of_day=43202000 --set roof_vent_offset_sign=Negative \
    --set roof_vent_offset_magnitude=12.5 --set windward_vent_position=37.5 \
    --set leeward_vent_position=0.0 --set vent_drive_state=Opening --set drive_current=850

sed -i.bak 's/"version": "1.0.0"/"version": "2.0.0"/' glasshouse-c/dictionary.json
rm glasshouse-c/dictionary.json.bak
run "$BITE" fmt --check glasshouse-c
run "$BITE" check --strict glasshouse-c
run "$BITE" lint glasshouse-c
run "$BITE" coverage --strict glasshouse-c
fails "$BITE" diff glasshouse glasshouse-c
run "$BITE" diff --check-version glasshouse glasshouse-c > /dev/null
echo "the version is raised by enough"

printf '\nretrace: ok\n'
