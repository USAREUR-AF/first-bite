# GZC-ICD-001 Revision C: Glasshouse Zone Controller GZC-4 Supervisory Link

Interface control document. **This document is synthetic.** The GZC-4 does not exist; the document was written for the first-bite worked example in `docs/icd-to-dictionary/06-worked-example/` and may be copied freely.

| | |
|---|---|
| Document | GZC-ICD-001 |
| Revision | C |
| Date | 2026-10-05 |
| Distribution | Unrestricted (synthetic) |

## 1 Scope

This document defines the messages exchanged between a GZC-4 glasshouse zone controller and its supervisor. A zone controller measures and regulates the climate and irrigation of one glasshouse zone. The supervisor collects the measurements of up to 32 zones and sends each controller its setpoints and commands.

## 2 Transport

Messages are carried in UDP datagrams over IPv4, one message per datagram.

| Interface | Direction | UDP port | Messages |
|---|---|---|---|
| ZONE | zone controller to supervisor | sent from port 47010 | section 5 |
| SUPV | supervisor to zone controller | sent to port 47011 | section 6 |

## 3 Data conventions

### 3.1 Words and bits

A message is a sequence of 16-bit words. Words are numbered from 1. Each word is sent most significant byte first. The bits of a word are numbered 15 to 0; bit 15 is the most significant and is sent first.

### 3.2 Fields

The tables give each field's word and its bits, most significant bit first: `12-5` is the eight bits from bit 12 down to bit 5. A field that spans words is listed with a range of words; its Bits column then gives the field's first bit in the first word and its last bit in the last word, and the field takes every bit between them. The most significant part is in the first word.

### 3.3 Types

| Type | Meaning |
|---|---|
| U | unsigned integer |
| S | signed integer, two's complement |
| OB | offset binary: the value is the unsigned integer minus half the field's range, so a field of n bits reads zero at 2^(n-1) |
| SM | sign-magnitude: the most significant bit is the sign (1 is negative), the remaining bits are the unsigned magnitude |
| E | enumeration; the values are in the table named in the description |
| D | discrete: one bit, 1 is set |
| A | ASCII characters, one per byte, first character in the most significant byte |
| — | spare |

### 3.4 Scaling

The LSB column gives the weight of the least significant bit. The value of a field is its integer multiplied by the LSB weight, in the unit of the Units column. For type OB the offset is subtracted before the multiplication.

### 3.5 Spare bits and reserved values

Spare bits are set to zero by the sender and ignored by the receiver. An enumeration value listed as Reserved is not sent; a receiver that gets one treats the field as invalid.

## 4 Message header

Every message of both interfaces begins with the four-word header of Table 4-1.

Table 4-1: Message header

| Field | Word | Bits | Type | LSB | Units | Description |
|---|---|---|---|---|---|---|
| Message ID | 1 | 15-8 | U | — | — | Identifies the message. See Table 5-1 and Table 6-1. |
| Message Length | 1 | 7-0 | U | 1 | words | Length of the message in 16-bit words. |
| Zone Number | 2 | 15-10 | U | — | — | Zone the message concerns, 1 to 32. |
| Sequence Number | 2 | 9-0 | U | — | — | Incremented by the sender for each message; wraps from 1023 to 0. |
| Time of Day | 3-4 | 15-0 | U | 1 | ms | Milliseconds since local midnight when the message was built. |

## 5 Zone controller to supervisor (ZONE)

### 5.1 Messages

Table 5-1: ZONE messages

| ID | Message | Length (words) | Sent | Defined in |
|---|---|---|---|---|
| 1 | Zone Status | 8 | 1 Hz | 5.2 |
| 2 | Climate Report | 11 | 0.2 Hz | 5.3 |
| 3 | Irrigation Report | 9 | 1 Hz during a run | 5.4 |
| 4 | Vent Report | 7 | 0.2 Hz | 5.5 |
| 5 | Alarm | 7 | on change | 5.6 |
| 6 | Event Log | 5 + 2N | on request | 5.7 |
| 7 | Maintenance Data | — | on request | GZC-MNT-004 |

Maintenance Data is defined in the maintenance manual GZC-MNT-004 and is not part of this document.

### 5.2 Zone Status

Sent once a second while the controller is powered. Words 1 to 4 are the message header (Table 4-1).

Table 5-2: Zone Status

| Field | Word | Bits | Type | LSB | Units | Description |
|---|---|---|---|---|---|---|
| Operating Mode | 5 | 15-12 | E | — | — | Mode the zone is in. See Table 5-2a. |
| Door Open | 5 | 11 | D | — | — | Set while the zone door is open. |
| Heating Active | 5 | 10 | D | — | — | Set while the heating valve is open. |
| Irrigation Active | 5 | 9 | D | — | — | Set while an irrigation run is in progress. |
| Alarm Present | 5 | 8 | D | — | — | Set while at least one alarm is unacknowledged. |
| Spare | 5 | 7-0 | — | — | — | Set to zero by the sender; ignored by the receiver. |
| Supply Voltage | 6 | 15-0 | U | 0.01 | V | Controller supply voltage. |
| Zone Label | 7-8 | 15-0 | A | — | — | Four ASCII characters, padded with spaces. |

Table 5-2a: Operating Mode values

| Value | Meaning |
|---|---|
| 0 | Off |
| 1 | Manual |
| 2 | Automatic |
| 3 | Frost Protection |
| 4 | Maintenance |
| 5 | Ventilation Purge |
| 6-15 | Reserved |

### 5.3 Climate Report

Sent every five seconds with the zone's air and weather measurements. Words 1 to 4 are the message header (Table 4-1).

Table 5-3: Climate Report

| Field | Word | Bits | Type | LSB | Units | Description |
|---|---|---|---|---|---|---|
| Air Temperature | 5 | 15-0 | S | 0.01 | °C | Air temperature at canopy height. |
| Relative Humidity | 6 | 15-6 | U | 0.1 | % | Relative humidity at canopy height. |
| Leaf Wetness | 6 | 5-0 | U | 1 | — | Leaf wetness index, a 6-bit value from 0 (dry) to 60 (saturated). |
| Outside Temperature | 7 | 15-4 | OB | 0.05 | °C | Outside air temperature, offset binary: zero is 2048. |
| Spare | 7 | 3-0 | — | — | — | Set to zero by the sender; ignored by the receiver. |
| CO2 Concentration | 8 | 15-0 | U | 1 | ppm | Carbon dioxide concentration. |
| Solar Irradiance | 9 | 15-0 | U | 0.1 | W/m² | Global irradiance above the canopy. |
| Wind Speed | 10 | 15-0 | U | 0.01 | m/s | Wind speed at the roof ridge. |
| Dew Point | 11 | 15-0 | S | 0.01 | °C | Dew point computed from air temperature and relative humidity. |

### 5.4 Irrigation Report

Sent once a second while an irrigation run is in progress, and once when it ends. Words 1 to 4 are the message header (Table 4-1).

Table 5-4: Irrigation Report

| Field | Word | Bits | Type | LSB | Units | Description |
|---|---|---|---|---|---|---|
| Spare | 5 | 15-12 | — | — | — | Set to zero by the sender; ignored by the receiver. |
| Valve 4 Open | 5 | 11 | D | — | — | Set while valve 4 is open. |
| Valve 3 Open | 5 | 10 | D | — | — | Set while valve 3 is open. |
| Valve 2 Open | 5 | 9 | D | — | — | Set while valve 2 is open. |
| Valve 1 Open | 5 | 8 | D | — | — | Set while valve 1 is open. |
| Pump State | 5 | 7-4 | E | — | — | State of the dosing pump. See Table 5-4a. |
| Spare | 5 | 3-0 | — | — | — | Set to zero by the sender; ignored by the receiver. |
| Flow Rate | 6 | 15-0 | U | 0.1 | L/min | Flow through the zone manifold. |
| Volume Delivered | 7-8 | 15-8 | U | 0.1 | L | Volume delivered since the run began, a 24-bit value. |
| Nutrient Solution pH | 8 | 7-0 | U | 0.1 | pH | Acidity of the nutrient solution, as pH. |
| Electrical Conductivity | 9 | 15-0 | U | 0.001 | mS/cm | Conductivity of the nutrient solution. |

Table 5-4a: Pump State values

| Value | Meaning |
|---|---|
| 0 | Stopped |
| 1 | Starting |
| 2 | Running |
| 3 | Fault |
| 4-15 | Reserved |

### 5.5 Vent Report

Sent every five seconds with the position of the roof vents. Words 1 to 4 are the message header (Table 4-1).

Table 5-5: Vent Report

| Field | Word | Bits | Type | LSB | Units | Description |
|---|---|---|---|---|---|---|
| Roof Vent Offset | 5 | 15-6 | SM | 0.1 | deg | Angle of the roof vents from the commanded angle, sign-magnitude: bit 15 is the sign (1 is negative), bits 14-6 the magnitude. |
| Spare | 5 | 5-0 | — | — | — | Set to zero by the sender; ignored by the receiver. |
| Windward Vent Position | 6 | 15-8 | U | 0.5 | % | Opening of the windward vents. |
| Leeward Vent Position | 6 | 7-0 | U | 0.5 | % | Opening of the leeward vents. |
| Vent Drive State | 7 | 15-12 | E | — | — | State of the vent drive. See Table 5-5a. |
| Drive Current | 7 | 11-0 | U | 1 | mA | Current drawn by the vent drive. |

Table 5-5a: Vent Drive State values

| Value | Meaning |
|---|---|
| 0 | Idle |
| 1 | Opening |
| 2 | Closing |
| 3 | Stalled |
| 4-14 | Reserved |
| 15 | Not Fitted |

### 5.6 Alarm

Sent when an alarm is raised, acknowledged or cleared. Words 1 to 4 are the message header (Table 4-1).

Table 5-6: Alarm

| Field | Word | Bits | Type | LSB | Units | Description |
|---|---|---|---|---|---|---|
| Alarm Code | 5 | 15-8 | E | — | — | What the alarm is. See Table 5-6a. |
| Severity | 5 | 7-6 | E | — | — | How serious the alarm is. See Table 5-6b. |
| Acknowledged | 5 | 5 | D | — | — | Set once an operator has acknowledged the alarm. |
| Spare | 5 | 4-0 | — | — | — | Set to zero by the sender; ignored by the receiver. |
| Occurrence Count | 6 | 15-0 | U | 1 | — | Times this alarm has been raised since midnight. |
| Minutes Since Raised | 7 | 15-5 | U | 1 | min | Whole minutes since the alarm was last raised. |
| Spare | 7 | 4-0 | — | — | — | Set to zero by the sender; ignored by the receiver. |

Table 5-6a: Alarm Code values

| Value | Meaning |
|---|---|
| 0 | Reserved |
| 1 | High Temperature |
| 2 | Low Temperature |
| 3 | High Humidity |
| 4 | Sensor Fault |
| 5 | Pump Fault |
| 6 | Vent Stalled |
| 7 | Power Supply |
| 8 | Low Battery |
| 9-199 | Reserved |
| 200-255 | Manufacturer specific |

Table 5-6b: Severity values

| Value | Meaning |
|---|---|
| 0 | Advisory |
| 1 | Caution |
| 2 | Warning |
| 3 | Reserved |

### 5.7 Event Log

Sent on request with the most recent events of the zone. Words 1 to 4 are the message header (Table 4-1).

Table 5-7: Event Log

| Field | Word | Bits | Type | LSB | Units | Description |
|---|---|---|---|---|---|---|
| Record Count | 5 | 15-8 | U | 1 | — | Number of event records that follow, 0 to 20. |
| Spare | 5 | 7-0 | — | — | — | Set to zero by the sender; ignored by the receiver. |

Record Count event records follow word 5, most recent first. Each is two words.

Table 5-7b: Event record

| Field | Word of the record | Bits | Type | LSB | Units | Description |
|---|---|---|---|---|---|---|
| Event Code | 1 | 15-8 | U | — | — | What happened. The codes are listed in GZC-MNT-004. |
| Device | 1 | 7-0 | U | — | — | Number of the device the event concerns. |
| Event Time | 2 | 15-0 | U | 2 | s | Seconds since local midnight, in steps of two seconds. |

## 6 Supervisor to zone controller (SUPV)

### 6.1 Messages

Table 6-1: SUPV messages

| ID | Message | Length (words) | Sent | Defined in |
|---|---|---|---|---|
| 1 | Setpoint Command | 8 | on change | 6.2 |
| 2 | Irrigation Command | 7 | on demand | 6.3 |
| 3 | Vent Command | 6 | on demand | 6.4 |
| 4 | Time Synchronisation | 7 | 1 per hour | 6.5 |

### 6.2 Setpoint Command

Sets the climate the zone holds in Automatic mode. Words 1 to 4 are the message header (Table 4-1).

Table 6-2: Setpoint Command

| Field | Word | Bits | Type | LSB | Units | Description |
|---|---|---|---|---|---|---|
| Day Temperature Setpoint | 5 | 15-0 | S | 0.01 | °C | Air temperature to hold between sunrise and sunset. |
| Night Temperature Setpoint | 6 | 15-0 | S | 0.01 | °C | Air temperature to hold between sunset and sunrise. |
| Humidity Setpoint | 7 | 15-6 | U | 0.1 | % | Relative humidity above which the vents open. |
| Spare | 7 | 5-0 | — | — | — | Set to zero by the sender; ignored by the receiver. |
| CO2 Setpoint | 8 | 15-0 | U | 1 | ppm | Carbon dioxide concentration to hold while the vents are closed. |

### 6.3 Irrigation Command

Starts an irrigation run on the valves selected. Words 1 to 4 are the message header (Table 4-1).

Table 6-3: Irrigation Command

| Field | Word | Bits | Type | LSB | Units | Description |
|---|---|---|---|---|---|---|
| Spare | 5 | 15-12 | — | — | — | Set to zero by the sender; ignored by the receiver. |
| Open Valve 4 | 5 | 11 | D | — | — | Set to include valve 4 in the run. |
| Open Valve 3 | 5 | 10 | D | — | — | Set to include valve 3 in the run. |
| Open Valve 2 | 5 | 9 | D | — | — | Set to include valve 2 in the run. |
| Open Valve 1 | 5 | 8 | D | — | — | Set to include valve 1 in the run. |
| Spare | 5 | 7-0 | — | — | — | Set to zero by the sender; ignored by the receiver. |
| Run Time | 6 | 15-0 | U | 1 | s | Longest time the run may last. |
| Volume Limit | 7 | 15-0 | U | 0.1 | L | Volume at which the run ends. |

### 6.4 Vent Command

Moves the roof vents. Words 1 to 4 are the message header (Table 4-1).

Table 6-4: Vent Command

| Field | Word | Bits | Type | LSB | Units | Description |
|---|---|---|---|---|---|---|
| Windward Vent Target | 5 | 15-8 | U | 0.5 | % | Opening to move the windward vents to. |
| Leeward Vent Target | 5 | 7-0 | U | 0.5 | % | Opening to move the leeward vents to. |
| Vent Command Mode | 6 | 15-12 | E | — | — | What the vent drive is to do. See Table 6-4a. |
| Spare | 6 | 11-0 | — | — | — | Set to zero by the sender; ignored by the receiver. |

Table 6-4a: Vent Command Mode values

| Value | Meaning |
|---|---|
| 0 | Hold |
| 1 | Go To Target |
| 2 | Close All |
| 3 | Storm Close |
| 4-15 | Reserved |

### 6.5 Time Synchronisation

Sets the controller's calendar and clock offset. Words 1 to 4 are the message header (Table 4-1).

Table 6-5: Time Synchronisation

| Field | Word | Bits | Type | LSB | Units | Description |
|---|---|---|---|---|---|---|
| Year | 5 | 15-9 | U | 1 | — | Years since 2000. |
| Month | 5 | 8-5 | U | 1 | — | Month of the year, 1 to 12. |
| Day | 5 | 4-0 | U | 1 | — | Day of the month, 1 to 31. |
| UTC Offset | 6 | 15-0 | S | 1 | min | Local time minus UTC. |
| Daylight Saving Active | 7 | 15 | D | — | — | Set while daylight saving time is in force. |
| Spare | 7 | 14-0 | — | — | — | Set to zero by the sender; ignored by the receiver. |

## Appendix A Summary of field types used

| Type | Used by |
|---|---|
| OB | Climate Report, Outside Temperature |
| SM | Vent Report, Roof Vent Offset |
| A | Zone Status, Zone Label |

## Appendix B Example messages

Each example gives a complete message as 16-bit words in hexadecimal, in the order sent, and the value of each field.

### B.1 Zone Status (ZONE)

```text
0108 0C11 0293 2E00 2500 096F 4E33 2020
```

| Field | Raw | Value |
|---|---|---|
| Message ID | 1 | 1 |
| Message Length | 8 | 8 words |
| Zone Number | 3 | 3 |
| Sequence Number | 17 | 17 |
| Time of Day | 43200000 | 43200000 ms |
| Operating Mode | 2 | Automatic |
| Door Open | 0 | Not set |
| Heating Active | 1 | Set |
| Irrigation Active | 0 | Not set |
| Alarm Present | 1 | Set |
| Supply Voltage | 2415 | 24.15 V |
| Zone Label | 0x4E332020 | "N3" |

### B.2 Climate Report (ZONE)

```text
020B 0C12 0293 31E8 FF06 CB6A 76D0 019F 0DEA 01A9 FDE8
```

| Field | Raw | Value |
|---|---|---|
| Message ID | 2 | 2 |
| Message Length | 11 | 11 words |
| Zone Number | 3 | 3 |
| Sequence Number | 18 | 18 |
| Time of Day | 43201000 | 43201000 ms |
| Air Temperature | -250 | -2.50 °C |
| Relative Humidity | 813 | 81.3 % |
| Leaf Wetness | 42 | 42 |
| Outside Temperature | 1901 | -7.35 °C |
| CO2 Concentration | 415 | 415 ppm |
| Solar Irradiance | 3562 | 356.2 W/m² |
| Wind Speed | 425 | 4.25 m/s |
| Dew Point | -536 | -5.36 °C |

### B.3 Irrigation Report (ZONE)

```text
0309 1FFF 0149 98F4 0520 007D 12D6 873E 073A
```

| Field | Raw | Value |
|---|---|---|
| Message ID | 3 | 3 |
| Message Length | 9 | 9 words |
| Zone Number | 7 | 7 |
| Sequence Number | 1023 | 1023 |
| Time of Day | 21600500 | 21600500 ms |
| Valve 4 Open | 0 | Not set |
| Valve 3 Open | 1 | Set |
| Valve 2 Open | 0 | Not set |
| Valve 1 Open | 1 | Set |
| Pump State | 2 | Running |
| Flow Rate | 125 | 12.5 L/min |
| Volume Delivered | 1234567 | 123456.7 L |
| Nutrient Solution pH | 62 | 6.2 |
| Electrical Conductivity | 1850 | 1.850 mS/cm |

### B.4 Vent Report (ZONE)

```text
0407 0C13 0293 35D0 9F40 4B00 1352
```

| Field | Raw | Value |
|---|---|---|
| Message ID | 4 | 4 |
| Message Length | 7 | 7 words |
| Zone Number | 3 | 3 |
| Sequence Number | 19 | 19 |
| Time of Day | 43202000 | 43202000 ms |
| Roof Vent Offset | 637 | -12.5 deg |
| Windward Vent Position | 75 | 37.5 % |
| Leeward Vent Position | 0 | 0.0 % |
| Vent Drive State | 1 | Opening |
| Drive Current | 850 | 850 mA |

### B.5 Alarm (ZONE)

```text
0507 3004 0038 CEFC 0580 0003 0FA0
```

| Field | Raw | Value |
|---|---|---|
| Message ID | 5 | 5 |
| Message Length | 7 | 7 words |
| Zone Number | 12 | 12 |
| Sequence Number | 4 | 4 |
| Time of Day | 3723004 | 3723004 ms |
| Alarm Code | 5 | Pump Fault |
| Severity | 2 | Warning |
| Acknowledged | 0 | Not set |
| Occurrence Count | 3 | 3 |
| Minutes Since Raised | 125 | 125 min |

### B.6 Event Log (ZONE)

```text
0609 0C14 0293 39B8 0200 1102 1234 1202 1240
```

| Field | Raw | Value |
|---|---|---|
| Message ID | 6 | 6 |
| Message Length | 9 | 9 words |
| Zone Number | 3 | 3 |
| Sequence Number | 20 | 20 |
| Time of Day | 43203000 | 43203000 ms |
| Record Count | 2 | 2 |

Words 6 to 9 are 2 event records.

### B.7 Setpoint Command (SUPV)

```text
0108 0C05 0149 9700 08CA 0640 AF00 0320
```

| Field | Raw | Value |
|---|---|---|
| Message ID | 1 | 1 |
| Message Length | 8 | 8 words |
| Zone Number | 3 | 3 |
| Sequence Number | 5 | 5 |
| Time of Day | 21600000 | 21600000 ms |
| Day Temperature Setpoint | 2250 | 22.50 °C |
| Night Temperature Setpoint | 1600 | 16.00 °C |
| Humidity Setpoint | 700 | 70.0 % |
| CO2 Setpoint | 800 | 800 ppm |

### B.8 Irrigation Command (SUPV)

```text
0207 1C06 0149 97FA 0500 0258 09C4
```

| Field | Raw | Value |
|---|---|---|
| Message ID | 2 | 2 |
| Message Length | 7 | 7 words |
| Zone Number | 7 | 7 |
| Sequence Number | 6 | 6 |
| Time of Day | 21600250 | 21600250 ms |
| Open Valve 4 | 0 | Not set |
| Open Valve 3 | 1 | Set |
| Open Valve 2 | 0 | Not set |
| Open Valve 1 | 1 | Set |
| Run Time | 600 | 600 s |
| Volume Limit | 2500 | 250.0 L |

### B.9 Vent Command (SUPV)

```text
0306 0C07 0293 2A18 6419 1000
```

| Field | Raw | Value |
|---|---|---|
| Message ID | 3 | 3 |
| Message Length | 6 | 6 words |
| Zone Number | 3 | 3 |
| Sequence Number | 7 | 7 |
| Time of Day | 43199000 | 43199000 ms |
| Windward Vent Target | 100 | 50.0 % |
| Leeward Vent Target | 25 | 12.5 % |
| Vent Command Mode | 1 | Go To Target |

### B.10 Time Synchronisation (SUPV)

```text
0407 0C08 0000 0000 356F FE20 0000
```

| Field | Raw | Value |
|---|---|---|
| Message ID | 4 | 4 |
| Message Length | 7 | 7 words |
| Zone Number | 3 | 3 |
| Sequence Number | 8 | 8 |
| Time of Day | 0 | 0 ms |
| Year | 26 | 26 |
| Month | 11 | 11 |
| Day | 15 | 15 |
| UTC Offset | -480 | -480 min |
| Daylight Saving Active | 0 | Not set |

## Appendix C Revision history

| Revision | Date | Change |
|---|---|---|
| A | 2026-03-02 | First issue. |
| B | 2026-06-15 | Added the Event Log message (5.7) and the Time Synchronisation message (6.5). |
| C | 2026-10-05 | 5.2: Operating Mode value 5, Ventilation Purge. 5.3: Leaf Wetness bits corrected to 5-0; Dew Point added as word 11, length now 11 words. 5.4: Flow Rate LSB changed from 0.25 to 0.1 L/min. 5.5: Motor Current renamed Drive Current. 5.6: Alarm Code value 8, Low Battery. |
