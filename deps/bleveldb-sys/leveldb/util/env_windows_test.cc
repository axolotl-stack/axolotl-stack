// Copyright (c) 2018 The LevelDB Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file. See the AUTHORS file for names of contributors.

#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include "gtest/gtest.h"
#include "leveldb/env.h"
#include "port/port.h"
#include "util/env_windows_test_helper.h"
#include "util/testutil.h"

namespace leveldb {

static const int kMMapLimit = 4;

class EnvWindowsTest : public testing::Test {
 public:
  static void SetFileLimits(int mmap_limit) {
    EnvWindowsTestHelper::SetReadOnlyMMapLimit(mmap_limit);
  }

  EnvWindowsTest() : env_(Env::Default()) {}

  Env* env_;
};

TEST_F(EnvWindowsTest, TestOpenOnRead) {
  // Write some test data to a single file that will be opened |n| times.
  std::string test_dir;
  ASSERT_LEVELDB_OK(env_->GetTestDirectory(&test_dir));
  std::string test_file = test_dir + "/open_on_read.txt";

  FILE* f = std::fopen(test_file.c_str(), "w");
  ASSERT_TRUE(f != nullptr);
  const char kFileData[] = "abcdefghijklmnopqrstuvwxyz";
  fputs(kFileData, f);
  std::fclose(f);

  // Open test file some number above the sum of the two limits to force
  // leveldb::WindowsEnv to switch from mapping the file into memory
  // to basic file reading.
  const int kNumFiles = kMMapLimit + 5;
  leveldb::RandomAccessFile* files[kNumFiles] = {0};
  for (int i = 0; i < kNumFiles; i++) {
    ASSERT_LEVELDB_OK(env_->NewRandomAccessFile(test_file, &files[i]));
  }
  char scratch;
  Slice read_result;
  for (int i = 0; i < kNumFiles; i++) {
    ASSERT_LEVELDB_OK(files[i]->Read(i, 1, &read_result, &scratch));
    ASSERT_EQ(kFileData[i], read_result[0]);
  }
  for (int i = 0; i < kNumFiles; i++) {
    delete files[i];
  }
  ASSERT_LEVELDB_OK(env_->RemoveFile(test_file));
}

TEST_F(EnvWindowsTest, RenameReplacesExistingFile) {
  std::string test_dir;
  ASSERT_LEVELDB_OK(env_->GetTestDirectory(&test_dir));
  const std::string source = test_dir + "/rename.dbtmp";
  const std::string target = test_dir + "/rename.CURRENT";
  ASSERT_LEVELDB_OK(WriteStringToFile(env_, "MANIFEST-000001\n", target));
  ASSERT_LEVELDB_OK(WriteStringToFile(env_, "MANIFEST-000002\n", source));

  ASSERT_LEVELDB_OK(env_->RenameFile(source, target));
  EXPECT_FALSE(env_->FileExists(source));
  std::string contents;
  ASSERT_LEVELDB_OK(ReadFileToString(env_, target, &contents));
  EXPECT_EQ("MANIFEST-000002\n", contents);
  ASSERT_LEVELDB_OK(env_->RemoveFile(target));
}

TEST_F(EnvWindowsTest, RenameDoesNotRequireSourceWriteSharing) {
  std::string test_dir;
  ASSERT_LEVELDB_OK(env_->GetTestDirectory(&test_dir));
  const std::string source = test_dir + "/shared-rename.dbtmp";
  const std::string target = test_dir + "/shared-rename.CURRENT";
  ASSERT_LEVELDB_OK(WriteStringToFile(env_, "MANIFEST-000001\n", target));
  ASSERT_LEVELDB_OK(WriteStringToFile(env_, "MANIFEST-000002\n", source));

  // A reader that permits deletion must not prevent renaming the source.
  // ReplaceFile unnecessarily requests write access to the replacement file.
  HANDLE reader = ::CreateFileA(
      source.c_str(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_DELETE,
      nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
  ASSERT_NE(INVALID_HANDLE_VALUE, reader);
  Status status = env_->RenameFile(source, target);
  char contents[16] = {};
  DWORD bytes_read = 0;
  const BOOL read =
      ::ReadFile(reader, contents, sizeof(contents), &bytes_read, nullptr);
  const BOOL closed = ::CloseHandle(reader);
  ASSERT_LEVELDB_OK(status);
  ASSERT_TRUE(read);
  ASSERT_TRUE(closed);
  EXPECT_EQ("MANIFEST-000002\n", std::string(contents, bytes_read));
  EXPECT_FALSE(env_->FileExists(source));
  std::string target_contents;
  ASSERT_LEVELDB_OK(ReadFileToString(env_, target, &target_contents));
  EXPECT_EQ("MANIFEST-000002\n", target_contents);
  ASSERT_LEVELDB_OK(env_->RemoveFile(target));
}

TEST_F(EnvWindowsTest, RenameCreatesTargetAndPreservesItOnMissingSource) {
  std::string test_dir;
  ASSERT_LEVELDB_OK(env_->GetTestDirectory(&test_dir));
  const std::string source = test_dir + "/new-rename.dbtmp";
  const std::string target = test_dir + "/new-rename.CURRENT";
  if (env_->FileExists(target)) {
    ASSERT_LEVELDB_OK(env_->RemoveFile(target));
  }
  ASSERT_LEVELDB_OK(WriteStringToFile(env_, "MANIFEST-000001\n", source));
  ASSERT_LEVELDB_OK(env_->RenameFile(source, target));
  EXPECT_FALSE(env_->FileExists(source));
  EXPECT_TRUE(env_->RenameFile(source, target).IsNotFound());
  std::string contents;
  ASSERT_LEVELDB_OK(ReadFileToString(env_, target, &contents));
  EXPECT_EQ("MANIFEST-000001\n", contents);
  ASSERT_LEVELDB_OK(env_->RemoveFile(target));
}

}  // namespace leveldb

int main(int argc, char** argv) {
  // All tests currently run with the same read-only file limits.
  leveldb::EnvWindowsTest::SetFileLimits(leveldb::kMMapLimit);
  testing::InitGoogleTest(&argc, argv);
  return RUN_ALL_TESTS();
}
