import * as anchor from "@coral-xyz/anchor";
import { Program, BN } from "@coral-xyz/anchor";
import { CreatorSupport } from "../target/types/creator_support";
import { expect } from "chai";
import { 
  TOKEN_PROGRAM_ID, 
  createMint, 
  getAssociatedTokenAddress, 
  mintTo, 
  createAssociatedTokenAccount,
  ASSOCIATED_TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountInstruction,
  createInitializeAccountInstruction,
  setAuthority
} from "@solana/spl-token";

describe("creator_support", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  
  const program = anchor.workspace.CreatorSupport as any;
  const adminKeypair = anchor.web3.Keypair.generate();
  const supporter = anchor.web3.Keypair.generate();

  let stablecoinMint: anchor.web3.PublicKey;
  let rewardMint: anchor.web3.PublicKey;
  let creatorSupportPDA: anchor.web3.PublicKey;
  let creatorPDA: anchor.web3.PublicKey;
  let vaultPDA: anchor.web3.PublicKey;

  before(async () => {
    // Fund admin keypair
    const airdropSignature = await provider.connection.requestAirdrop(
      adminKeypair.publicKey,
      2e9 // 2 SOL
    );
    await provider.connection.confirmTransaction(airdropSignature, "confirmed");

    // Create mints
    stablecoinMint = await createMint(
      provider.connection,
      adminKeypair,
      adminKeypair.publicKey,
      null,
      6
    );

    rewardMint = await createMint(
      provider.connection,
      adminKeypair,
      adminKeypair.publicKey,  // Initial mint authority
      null,
      6
    );

    // Transfer mint authority to PDA
    const [mintAuthPDA] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("mint_auth")],
      program.programId
    );

    await setAuthority(
      provider.connection,
      adminKeypair,
      rewardMint,
      adminKeypair.publicKey,
      0,  // AuthorityType.MintTokens
      mintAuthPDA,
      []
    );

    // Derive CreatorSupport PDA
    [creatorSupportPDA] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("creator_support")],
      program.programId
    );
  });

  it("Initializes program", async () => {
    // Derive mint authority PDA
    const [mintAuthPDA] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("mint_auth")],
      program.programId
    );

    const tx = await program.methods
      .initialize(
        new BN(100),   // price_per_impact
        new BN(150),   // max_reward_multiplier
        new BN(50),    // scaling_factor
        new BN(1000),  // apr (10%)
        new BN(70),    // supporter_reward_ratio
        new BN(1_000_000)  // min_stake_amount (1 token with 6 decimals)
      )
      .accounts({
        creatorSupport: creatorSupportPDA,
        admin: adminKeypair.publicKey,
        mintAuthority: mintAuthPDA,
        rewardMint: rewardMint,
        stablecoinMint: stablecoinMint,
        systemProgram: anchor.web3.SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .signers([adminKeypair])
      .rpc();

    console.log("Initialization tx:", tx);
    
    // Verify account creation
    const account = await program.account.creatorSupport.fetch(creatorSupportPDA);
    expect(account.pricePerImpact.toNumber()).to.equal(100);
    expect(account.maxRewardMultiplier.toNumber()).to.equal(150);
    expect(account.scalingFactor.toNumber()).to.equal(50);
  });


  it("Supports a creator", async () => {
    // Derive Creator PDA
    [creatorPDA] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("creator"), adminKeypair.publicKey.toBuffer()],
      program.programId
    );

    // Initialize Creator
    const initCreatorTx = await program.methods
      .initializeCreator()
      .accounts({
        creator: creatorPDA,
        admin: adminKeypair.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([adminKeypair])
      .rpc();

    console.log("Initialize Creator tx:", initCreatorTx);

    // Derive Vault PDA
    [vaultPDA] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("vault"), creatorPDA.toBuffer()],
      program.programId
    );

    // Initialize Vault with correct authority
    const vaultTokenAccountKp = anchor.web3.Keypair.generate();

    // Create token account for vault with PDA as authority
    const createVaultTokenAccountIx = anchor.web3.SystemProgram.createAccount({
      fromPubkey: adminKeypair.publicKey,
      newAccountPubkey: vaultTokenAccountKp.publicKey,
      space: 165,
      lamports: await provider.connection.getMinimumBalanceForRentExemption(165),
      programId: TOKEN_PROGRAM_ID,
    });

    const initVaultTokenAccountIx = createInitializeAccountInstruction(
      vaultTokenAccountKp.publicKey,
      stablecoinMint,
      vaultPDA,
      TOKEN_PROGRAM_ID
    );

    const init_tx = new anchor.web3.Transaction()
      .add(createVaultTokenAccountIx)
      .add(initVaultTokenAccountIx);

    await provider.sendAndConfirm(init_tx, [adminKeypair, vaultTokenAccountKp]);

    // Create and fund supporter's stablecoin ATA
    const supporterStablecoinATA = await getAssociatedTokenAddress(
      stablecoinMint,
      supporter.publicKey
    );

    const createSupporterATA = await createAssociatedTokenAccount(
      provider.connection,
      adminKeypair,
      stablecoinMint,
      supporter.publicKey
    );

    console.log("Supporter's Stablecoin ATA:", createSupporterATA);

    // Mint stablecoins to supporter
    const mintToTx = await mintTo(
      provider.connection,
      adminKeypair,
      stablecoinMint,
      supporterStablecoinATA,
      adminKeypair.publicKey,
      100_000_000 // 100 tokens (6 decimals)
    );

    console.log("Minted Stablecoins to Supporter tx:", mintToTx);

    // Create creator's stablecoin ATA
    const creatorStablecoinATA = await getAssociatedTokenAddress(
      stablecoinMint,
      creatorPDA,
      true  // allowOwnerOffCurve - needed for PDA
    );

    // Create ATA for PDA using createAssociatedTokenAccountInstruction
    const createATAIx = createAssociatedTokenAccountInstruction(
      adminKeypair.publicKey,  // payer
      creatorStablecoinATA,    // ata
      creatorPDA,              // owner (PDA)
      stablecoinMint,         // mint
      TOKEN_PROGRAM_ID,
      ASSOCIATED_TOKEN_PROGRAM_ID
    );

    const tx = new anchor.web3.Transaction().add(createATAIx);
    await provider.sendAndConfirm(tx, [adminKeypair]);

    console.log("Creator's Stablecoin ATA:", creatorStablecoinATA.toString());

    // Fund supporter with SOL
    const supporterAirdropSig = await provider.connection.requestAirdrop(
      supporter.publicKey,
      2e9
    );
    await provider.connection.confirmTransaction({
      signature: supporterAirdropSig,
      blockhash: (await provider.connection.getLatestBlockhash()).blockhash,
      lastValidBlockHeight: (await provider.connection.getLatestBlockhash()).lastValidBlockHeight,
    });

    // Execute support_creator
    const [stakePDA] = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("stake"), supporter.publicKey.toBuffer(), creatorPDA.toBuffer()],
      program.programId
    );

    console.log("Stake PDA:", stakePDA.toString());

      const supportCreatorTx = await program.methods
        .supportCreator(new BN(10_000_000))
        .accounts({
          creatorSupport: creatorSupportPDA,
          creator: creatorPDA,
          supporter: supporter.publicKey,
          supporterStake: stakePDA,
          systemProgram: anchor.web3.SystemProgram.programId,
          stablecoinMint: stablecoinMint,
          supporterStablecoinAccount: supporterStablecoinATA,
          creatorStablecoinAccount: creatorStablecoinATA,
          vaultAccount: vaultTokenAccountKp.publicKey,
          vaultPda: vaultPDA,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([supporter])
        .rpc();

      console.log("Support Creator tx:", supportCreatorTx);

    // Fetch and verify Creator account
    const creatorAccount = await program.account.creator.fetch(creatorPDA);
    expect(creatorAccount.totalSupportAmount.toNumber()).to.equal(7_000_000); // 70% of 10
    expect(creatorAccount.totalStaked.toNumber()).to.equal(3_000_000); // 30% of 10
  });


  it("Claims rewards", async () => {
    try {
      // Advance time by 30 days to accumulate more rewards
      const advanceTimeTx = await program.methods
        .advanceTime(new BN(30 * 24 * 60 * 60)) // 30 days in seconds
        .accounts({
          clock: anchor.web3.SYSVAR_CLOCK_PUBKEY,
        })
        .rpc();

      console.log("Advance Time tx:", advanceTimeTx);

      // Derive stake PDA
      const [stakePDA] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("stake"), supporter.publicKey.toBuffer(), creatorPDA.toBuffer()],
        program.programId
      );

      console.log("Stake PDA:", stakePDA.toString());

      // Derive mint authority PDA
      const [mintAuthPDA] = anchor.web3.PublicKey.findProgramAddressSync(
        [Buffer.from("mint_auth")],
        program.programId
      );

      console.log("Mint Authority PDA:", mintAuthPDA.toString());

      // Derive supporter reward ATA
      const supporterRewardATA = await getAssociatedTokenAddress(
        rewardMint,
        supporter.publicKey
      );

      console.log("Supporter Reward ATA:", supporterRewardATA.toString());
      // Derive creator reward ATA
      const creatorRewardATA = await getAssociatedTokenAddress(
        rewardMint,
        creatorPDA,
        true  // allowOwnerOffCurve - needed for PDA
      );

      console.log("Creator Reward ATA:", creatorRewardATA.toString());

      // Create ATAs if they don't exist
      try {
        await createAssociatedTokenAccount(
          provider.connection,
          adminKeypair,
          rewardMint,
          supporter.publicKey
        );

        console.log("Created Supporter Reward ATA");

        const createCreatorRewardATAIx = createAssociatedTokenAccountInstruction(
          adminKeypair.publicKey,  // payer
          creatorRewardATA,        // ata
          creatorPDA,              // owner (PDA)
          rewardMint,              // mint
          TOKEN_PROGRAM_ID,
          ASSOCIATED_TOKEN_PROGRAM_ID
        );

        console.log("Created Creator Reward ATA");

        await provider.sendAndConfirm(
          new anchor.web3.Transaction().add(createCreatorRewardATAIx),
          [adminKeypair]
        );
      } catch (e) {
        // ATAs might already exist, continue
        console.log("ATAs might already exist:", e);
      }

      // Execute claim_rewards
      const claimRewardsTx = await program.methods
        .claimRewards()
        .accounts({
          supporterStake: stakePDA,
          creator: creatorPDA,
          supporter: supporter.publicKey,
          creatorSupport: creatorSupportPDA,
          rewardMint: rewardMint,
          supporterRewardAccount: supporterRewardATA,
          creatorRewardAccount: creatorRewardATA,
          mintAuthority: mintAuthPDA,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .signers([supporter])
        .rpc();

      console.log("Claim Rewards tx:", claimRewardsTx);

      // Fetch and verify reward balances
      const supporterBalance = await provider.connection.getTokenAccountBalance(supporterRewardATA);
      const creatorBalance = await provider.connection.getTokenAccountBalance(creatorRewardATA);
      
      console.log("Supporter rewards:", supporterBalance.value.amount);
      console.log("Creator rewards:", creatorBalance.value.amount);
      
      expect(Number(supporterBalance.value.amount)).to.be.greaterThan(0);
      expect(Number(creatorBalance.value.amount)).to.be.greaterThan(0);

      // Add debug logs for reward calculation
      const stake = await program.account.supporterStake.fetch(stakePDA);
      const creatorSupport = await program.account.creatorSupport.fetch(creatorSupportPDA);
      
      console.log("Stake amount:", stake.stakedAmount.toString());
      console.log("APR:", creatorSupport.apr.toString());
      console.log("Last claim time:", stake.lastClaimTime.toString());
      console.log("Current time:", (await provider.connection.getBlockTime(await provider.connection.getSlot())).toString());
    } catch (error) {
      console.log("Error:", error);
      throw error;
    }
  });
});
