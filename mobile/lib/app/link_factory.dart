// Picks the DeviceLink: the in-memory fake with --dart-define=FAKE_LINK=true,
// otherwise the native platform-channel link.
import '../link/device_link.dart';
import '../link/device_link_channel.dart';
import '../link/fake_device_link.dart';

const bool useFakeLink = bool.fromEnvironment('FAKE_LINK');

DeviceLink createDeviceLink() => useFakeLink ? FakeDeviceLink() : DeviceLinkChannel();
