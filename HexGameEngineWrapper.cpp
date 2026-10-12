#include "HexGameEngineWrapper.h"

#if defined(_WIN32)
#define HGS_DLL_IMPORT __declspec(dllimport)
#else
#define HGS_DLL_IMPORT
#endif

extern "C"
{
	HGS_DLL_IMPORT void hgs_initialize_test_match(const char* c_match_id);
	HGS_DLL_IMPORT char* hgs_get_current_state(const char* c_match_id);
	HGS_DLL_IMPORT char* hgs_process_token_for_match(const char* c_match_id, const char* c_token);
	HGS_DLL_IMPORT void hgs_free_string(char* rust_string);
}

#undef HGS_DLL_IMPORT

void UHexGameEngineWrapper::InitializeTestMatch(FString MatchID)
{
	FTCHARToUTF8 MatchIDUtf8(*MatchID);
	hgs_initialize_test_match(MatchIDUtf8.Get());
}

FString UHexGameEngineWrapper::GetCurrentState(FString MatchID)
{
	FTCHARToUTF8 MatchIDUtf8(*MatchID);
	char* RawPtr = hgs_get_current_state(MatchIDUtf8.Get());
	if (RawPtr == nullptr)
	{
		return TEXT("E:No active match or state unavailable");
	}

	FString Result(UTF8_TO_TCHAR(RawPtr));
	hgs_free_string(RawPtr);
	return Result;
}

FString UHexGameEngineWrapper::ProcessTokenForMatch(FString MatchID, FString Token)
{
	FTCHARToUTF8 MatchIDUtf8(*MatchID);
	FTCHARToUTF8 TokenUtf8(*Token);
	char* RawPtr = hgs_process_token_for_match(MatchIDUtf8.Get(), TokenUtf8.Get());
	if (RawPtr == nullptr)
	{
		return TEXT("E:Rust core returned a null response");
	}

	FString Result(UTF8_TO_TCHAR(RawPtr));
	hgs_free_string(RawPtr);
	return Result;
}