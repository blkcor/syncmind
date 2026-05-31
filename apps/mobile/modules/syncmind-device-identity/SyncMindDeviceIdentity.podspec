Pod::Spec.new do |s|
  s.name           = 'SyncMindDeviceIdentity'
  s.version        = '0.0.1'
  s.summary        = 'Native secure device identity module for SyncMind mobile.'
  s.description    = 'Stores and uses the mobile Ed25519 identity through native secure storage.'
  s.author         = 'SyncMind'
  s.homepage       = 'https://syncmind.local'
  s.platforms      = { :ios => '15.1' }
  s.source         = { :git => '' }
  s.static_framework = true

  s.dependency 'ExpoModulesCore'
  s.source_files = 'ios/**/*.{h,m,mm,swift}'
end
