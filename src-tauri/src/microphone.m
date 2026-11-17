#import <AVFoundation/AVFoundation.h>
#include <string.h>

static AVAudioRecorder *recorder;
static bool permissionRequested;

int soliton_mic_permission(void) {
    AVAuthorizationStatus status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
    if (status == AVAuthorizationStatusNotDetermined) {
        if (!permissionRequested) {
            permissionRequested = true;
            [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL granted) {}];
        }
        return 0;
    }
    return status == AVAuthorizationStatusAuthorized ? 1 : -1;
}

bool soliton_mic_start(void) {
    if (recorder) return false;
    NSURL *url = [NSFileManager.defaultManager.temporaryDirectory
        URLByAppendingPathComponent:[NSUUID.UUID.UUIDString stringByAppendingPathExtension:@"wav"]];
    NSDictionary *settings = @{
        AVFormatIDKey: @(kAudioFormatLinearPCM),
        AVSampleRateKey: @16000,
        AVNumberOfChannelsKey: @1,
        AVLinearPCMBitDepthKey: @16,
        AVLinearPCMIsFloatKey: @NO,
        AVLinearPCMIsBigEndianKey: @NO
    };
    recorder = [[AVAudioRecorder alloc] initWithURL:url settings:settings error:NULL];
    if ([recorder recordForDuration:30]) return true;
    [recorder deleteRecording];
    recorder = nil;
    return false;
}

size_t soliton_mic_stop(bool keep, uint8_t *out, size_t capacity) {
    if (!recorder) return 0;
    [recorder stop];
    NSData *recorded = keep ? [NSData dataWithContentsOfURL:recorder.url] : nil;
    [recorder deleteRecording];
    recorder = nil;
    size_t length = recorded.length;
    if (length > capacity) length = 0;
    if (length) memcpy(out, recorded.bytes, length);
    return length;
}
