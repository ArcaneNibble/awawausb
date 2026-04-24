# Physical devices and your browser: an overview

There are currently many different ways that web pages can talk to and use devices connected to your computer:

- WebUSB
- WebHID
- Web Serial
- Web MIDI
- Web NFC
- Web Bluetooth
- cameras and microphones
- Gamepad API
- "WiFi" (standard computer networking)
- "universal" peripherals such as a mouse, keyboard, or screen

Confusingly, despite how many of these devices today use USB _physical connectors_ to plug into your computer, devices which fall under the "more specific" categories cannot (or in practice do not, even if they could) use _WebUSB_ as the programming interface.

In general, the designers of these APIs have attempted to force other developers to choose only the "most appropriate" way to interact with devices. Often, this is because the designers of these APIs know that both users and programmers hold implicit assumptions about how devices should work. However, it is incredibly difficult to write down what these assumptions actually _are_ (see: the remainder of this document), so "following what has been done before" appears to become the simplest solution.

Unfortunately, the differences between these interfaces exists primarily for the convenience of (different groups of) _computer programmers_. It is not a natural nor inherent property of the devices themselves. Figuring out "what is going on" thus requires understanding how _programmers_ understand the hardware.

## "Universal" computer peripherals

Some devices, primarily the mouse, keyboard, and screen/display/monitor, have been associated with personal computers for so long that they are usually assumed to be always present. (The mouse is the newest and least established of these, and some nitpicky arguments might be made around the "mouse" vs "trackpad" vs "touchscreen" vs other types of "pointing devices".)

Web pages always have access to these devices without asking you, because these devices follow "the usual rules" of the "desktop" metaphor for personal computing. Software developers often assume that this metaphor has been around for so long that it can be considered universal knowledge.

For example, a web page does not need to ask you for permission to show you text or pictures. If you do not want to see the text or pictures, you can close the page, minimize it, or cover it up with a different program. A web page can track everything you type on the keyboard and every movement you make with the mouse, but only if the web page "has focus" because you have selected it to interact with. If you select a different web page or program, the web page you have moved away from automatically cannot receive keyboard and mouse inputs anymore.

However, not every "thing that has buttons which can be pressed" is a "keyboard", and not every "device for inputting relative motion" is a "mouse" or "pointing devices". Sometimes, devices which do _not_ have these physical shapes _are_ nonetheless designed to be treated as keyboards or mice (for example, barcode scanners are typically keyboards). The specific choice is made by the designer of the device, usually in an attempt to get a specific outcome the designer wants within the constraints of the ambient behavior of the computing ecosystem _at the time_.

## "Generic" computer peripherals

Some peripherals have been associated with computers for a long time and are considered to have "well-understood" behaviors, even if not all computers have them.

Examples of this category of device include cameras (specifically, "webcams"), microphones, printers, and network adapters.

Because programmers believe that these device behaviors are well-defined, they have designed specific interfaces for them:

- requesting to use your camera
- requesting to use your microphone
- asking the browser to print the web page ([yes, really!](https://developer.mozilla.org/en-US/docs/Web/API/Window/print))
- making requests to other computers

## Human Interface Devices

A generic expression used for describing "something which allows a user to interact with a computer" is "human interface device" (HID). This term was either invented or popularized by the development of the USB standard during the 1990s.

During this time, there was significant experimentation around interacting with personal computers. Some examples include [ergonomic keyboards](https://en.wikipedia.org/wiki/Ergonomic_keyboard) and a [huge variety](https://en.wikipedia.org/wiki/Game_controller#Variants) of controllers for playing games. These devices all connected to the computer in [different](https://en.wikipedia.org/wiki/PS/2_port) and [incompatible](https://en.wikipedia.org/wiki/Game_port) ways. Worse, different types of computers had different interfaces as well (devices, _even keyboards_, for an [IBM PC compatible](https://en.wikipedia.org/wiki/IBM_PC_compatible) were [not compatible](https://en.wikipedia.org/wiki/Apple_Desktop_Bus) with the [Apple Macintosh](<https://en.wikipedia.org/wiki/Mac_(computer)#1984%E2%80%931991:_Launch_and_early_success>)).

One of the goals of USB was to unify all of these devices into a single, flexible standard. This would make designing and programming simpler, which would result in more, cheaper, and less confusing devices for consumers (well, supposedly at least).

In practice, a number of things happened:

- "Universal" peripherals such as mice and keyboards did adapt well to USB HID (although these were rapidly becoming cheap commodity products either way). Value-added features were introduced by making use of parts of the standard specifically designed for such.
- Game controllers converged around approximately-one single successful "gamepad" design, relegating other designs (for example, racing or flight simulators) to niche hobbies. Although this often uses USB, [other market forces](<https://en.wikipedia.org/wiki/Halo_(franchise)>) pushed some (but not all) controllers [away from](https://learn.microsoft.com/en-us/windows/win32/xinput/xinput-game-controller-apis-portal) the USB HID specification.
- Designers of "simple" devices realized that the USB HID specification was designed generically enough to be useful for their devices, even if their devices have nothing to do with interfacing with humans at all (for example, [uninterruptible power supplies](https://en.wikipedia.org/wiki/Uninterruptible_power_supply) or environmental monitoring sensors). Many of these devices intentionally take advantage of various historically "looser" security policies around "input" devices compared to other types of hardware.

Finally, technological change caused the HID specification to be reused for non-USB devices. This reuse allows programmers to use existing code, again saving development costs. This is possible because many large software systems are built out of [abstract "layers"](https://en.wikipedia.org/wiki/Abstraction_layer) which can be separated and recombined in new ways.

For example, "HID" has been reused for [Bluetooth](https://en.wikipedia.org/wiki/Bluetooth) devices, [Bluetooth Low Energy](https://en.wikipedia.org/wiki/Bluetooth_Low_Energy) devices, and [internal](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/hid-over-spi) [sub-components](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/hid-over-i2c-guide) of highly-integrated computers such as laptops and tablets. This means that "HID" does _not_ imply "USB".

One _very important_ observation is that "universal" computer peripherals such as keyboards and mice predate USB and HID. As described in the [above section](#universal-computer-peripherals), cultural assumptions about how _these specific_ devices should interact with the "desktop" metaphor had already been established long before USB HID was invented.

So, as a result of all of this (note that every guideline in this list has its exceptions):

- Keyboards and mice are "very special" and specifically blocked by programming interfaces such as WebHID, because of the longstanding assumptions about how they are supposed to interact with concepts such as "windows" and "focus" and "the desktop"
  - This includes devices (such as barcode scanners or drawing tablets) that act like keyboards or mice to programmers, even if they don't physically look like keyboards or mice.
  - Depending on the fine details of their programming, this _may or may not_ include value-added features. For example, a "caps lock" LED is treated as special and protected (because it existed on PC keyboards since the 80s), but "gamer RGB" LEDs are not treated as special (because they are a much more recent invention).
- If a device is related to playing games, it may or may not be HID, and/or the device may have a configuration setting to specify whether or not it should use HID.
  - If it uses HID, a web page can use WebHID to access it
  - In either case, a web page can use the [Gamepad API](https://developer.mozilla.org/en-US/docs/Web/API/Gamepad_API) to access it
- WebHID is the preferred interface for specifically accessing _nonstandard_ features or features _not_ common to the majority of buttons-and-thumbsticks gamepads
  - For example, _changing the settings_ of a keyboard or controller

## Serial ports

Serial ports are an extremely old and (arguably not) simple communications interface that long predates personal computers (or [computers at all](https://en.wikipedia.org/wiki/Baudot_code)).

During the early history of the computer, [teletypes](https://en.wikipedia.org/wiki/Teleprinter), an already-existing technology, were often used to interact with them. A long, continuous history of reusing existing infrastructure then followed, even as cheaper technology made personal computers possible. For example, personal computers could connect to large, expensive, shared computers using [modems](https://en.wikipedia.org/wiki/Modem) and the telephone network… often by behaving like a teletype. This led to most computers (both personal computers and otherwise) having serial ports.

During the personal computer era (and before the invention of USB), the serial port was itself reused for various peripherals, such as [mice](https://en.wikipedia.org/wiki/Microsoft_Mouse), [PDAs](https://en.wikipedia.org/wiki/Personal_digital_assistant), [early home automation](<https://en.wikipedia.org/wiki/X10_(industry_standard)>), etc.

Although USB eventually replaced serial ports for "consumer" peripherals due to better features such as user-friendliness, serial ports continue to exist in more specialized applications due to their simplicity. For backwards-compatibility, there are adapters that convert between serial ports and USB.

Nowadays, the things still using serial ports are typically not-mass-market devices particularly with cultural connections to "industry" or "telecommunications". When using WebSerial, frequently encountered devices include 3d printers and microcontroller development boards.

## MIDI

MIDI, or Musical Instrument Digital Interface, is an old standard for allowing electronic musical instruments to interoperate.

The popularization of MIDI happened at the same time as the growth of personal computers, and so the technology faced similar incentives such as wanting to reuse code and interfaces. However, MIDI successfully managed to clearly define and dominate one single niche (electronic music).

As a result, even though MIDI reused technology such as "serial ports" and later adapted to using USB, its user base continues to remain culturally distinct. It has managed to resist many of the trends seen elsewhere in computers and consumer electronics.

Nowadays, USB devices that are somehow related to "making music" (and _only_ these devices) are probably MIDI devices. On the web, these devices use the Web MIDI API.

## NFC

NFC, or near-field communication, is a term for wirelessly exchanging information with a device that is physically close to another device.

Although this term can sometimes be used in very generic ways, "Web NFC" describes one particular highly-limited API that specifically allows web pages on browsers (i.e. Chrome) on mobile phones (i.e. Android) to interact with a very specific subset of NFC devices (NDEF tags). Essentially, this was designed to enable only certain "physical interaction" applications that Google wanted.

Although it is possible to purchase an NFC reader for a "computer" (i.e. _not_ a mobile phone), and although some laptops have NFC readers built-in, it is not possible to use these with Web NFC. It is _also_ not possible to use these with WebUSB, even though most of these readers physically connect using USB.

The reason for the above limitation is because of the cultural link between NFC technologies and applications which involve large centralized power structures (e.g. [payments](https://en.wikipedia.org/wiki/EMV), [public transit](https://en.wikipedia.org/wiki/MIFARE#Transportation), [physical access control](https://en.wikipedia.org/wiki/Proximity_card), and [government identification documents](https://en.wikipedia.org/wiki/Biometric_passport)). This has caused NFC support on non-mobile-phone computers to become thought of as a highly-specialized capability often used by powerful organizations (who would not want the tradeoffs and added risks that would come with exposing this capability to web pages).

In short, proximity tags on mobile phones is a _very unusual anomaly_ within the larger ecosystem.

If you do have unusual requirements as mentioned above, NFC smart cards _can_ be exposed to web pages through [PKCS #11](https://en.wikipedia.org/wiki/PKCS_11). This is an older technology related to managing "cryptography" in a manner that institutions understand and is not a "web" technology.

## Bluetooth

Bluetooth is the name for _two_ mostly-unrelated wireless technologies, Bluetooth Classic and Bluetooth Low Energy. These technologies attempted to define standards for wireless communications between portable devices.

Despite being called "Web Bluetooth", the Web Bluetooth API is designed for Bluetooth Low Energy _only_, specifically the Generic Attribute Profile (GATT).

Although Bluetooth as a technology is only slightly newer than USB, and applications such as wireless headsets have been available for a long time, the complexity and costs slowed down its early adoption. Adoption increased dramatically after the popularization of mobile phones.

Nowadays, wireless devices which are designed to be used with a mobile phone or tablet most likely use Bluetooth, specifically Bluetooth Low Energy. However, as mentioned above, [input devices are special](#human-interface-devices).
