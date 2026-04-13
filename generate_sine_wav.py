#!/usr/bin/env python3
import wave
import struct
import math

# Parameters
frequency = 440  # Hz (A4)
sample_rate = 48000  # Hz
duration = 4  # seconds
channels = 1  # Mono
sample_width = 2  # 16-bit = 2 bytes

# Output file path
output_file = "/home/synth/source/pirate-synth/sdcard/boot/firmware/pirate-synth/WAV/sine.wav"

# Calculate number of samples
num_samples = int(sample_rate * duration)

# Open WAV file for writing
with wave.open(output_file, 'wb') as wav_file:
    # Set WAV parameters
    wav_file.setnchannels(channels)
    wav_file.setsampwidth(sample_width)
    wav_file.setframerate(sample_rate)
    
    # Generate and write sine wave samples
    for i in range(num_samples):
        # Calculate sine value
        angle = 2.0 * math.pi * frequency * i / sample_rate
        sine_value = math.sin(angle)
        
        # Convert to 16-bit PCM (-32768 to 32767)
        sample = int(sine_value * 32767)
        
        # Pack as little-endian signed short
        data = struct.pack('<h', sample)
        wav_file.writeframes(data)

print(f"Sine wave WAV file generated: {output_file}")
print(f"Frequency: {frequency} Hz")
print(f"Sample rate: {sample_rate} Hz")
print(f"Duration: {duration} seconds")
print(f"Channels: Mono")
print(f"Sample width: 16-bit PCM")
print(f"Total samples: {num_samples}")
