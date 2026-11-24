#import <AVFoundation/AVFoundation.h>
#include <stdbool.h>

// Runs on the export worker. The synchronous API also supports macOS 12.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
bool grove_movie_matches(const char *path, const char *identifier) {
    @autoreleasepool {
        NSURL *url = [NSURL fileURLWithFileSystemRepresentation:path isDirectory:NO relativeToURL:nil];
        NSDictionary *options = @{AVURLAssetReferenceRestrictionsKey: @(AVAssetReferenceRestrictionForbidAll)};
        AVURLAsset *asset = [AVURLAsset URLAssetWithURL:url options:options];
        if (![asset tracksWithMediaType:AVMediaTypeVideo].count) return false;
        NSArray *items = [AVMetadataItem metadataItemsFromArray:[asset metadataForFormat:AVMetadataFormatQuickTimeMetadata]
                                           filteredByIdentifier:AVMetadataIdentifierQuickTimeMetadataContentIdentifier];
        if (items.count != 1) return false;
        NSString *value = [items.firstObject stringValue];
        if (!value) return false;
        NSUUID *actual = [[NSUUID alloc] initWithUUIDString:value];
        NSUUID *expected = [[NSUUID alloc] initWithUUIDString:[NSString stringWithUTF8String:identifier]];
        return [actual isEqual:expected];
    }
}
#pragma clang diagnostic pop
