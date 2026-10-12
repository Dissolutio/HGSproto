#pragma once

#include "CoreMinimal.h"
#include "Kismet/BlueprintFunctionLibrary.h"
#include "HexGameEngineWrapper.generated.h"

UCLASS()
class UHexGameEngineWrapper final : public UBlueprintFunctionLibrary
{
	GENERATED_BODY()

public:
	UFUNCTION(BlueprintCallable, Category = "HexGameSim")
	static void InitializeTestMatch(FString MatchID);

	UFUNCTION(BlueprintCallable, Category = "HexGameSim")
	static FString GetCurrentState(FString MatchID);

	UFUNCTION(BlueprintCallable, Category = "HexGameSim")
	static FString ProcessTokenForMatch(FString MatchID, FString Token);

	UFUNCTION(BlueprintCallable, Category = "HexGameSim")
	static FString HostMatch(FString MatchID);

	UFUNCTION(BlueprintCallable, Category = "HexGameSim")
	static bool JoinMatch(FString MatchID, FString HostNodeID);
};