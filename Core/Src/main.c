/* USER CODE BEGIN Header */
/**
  ******************************************************************************
  * @file           : main.c
  * @brief          : Main program body
  ******************************************************************************
  * @attention
  *
  * Copyright (c) 2026 STMicroelectronics.
  * All rights reserved.
  *
  * This software is licensed under terms that can be found in the LICENSE file
  * in the root directory of this software component.
  * If no LICENSE file comes with this software, it is provided AS-IS.
  *
  ******************************************************************************
  */
/* USER CODE END Header */
/* Includes ------------------------------------------------------------------*/
#include "main.h"
#include "dfsdm.h"
#include "dma.h"
#include "usart.h"
#include "gpio.h"

/* Private includes ----------------------------------------------------------*/
/* USER CODE BEGIN Includes */
#include <string.h>   /* memcpy — used in DFSDM DMA callbacks */
/* USER CODE END Includes */

/* Private typedef -----------------------------------------------------------*/
/* USER CODE BEGIN PTD */

/* USER CODE END PTD */

/* Private define ------------------------------------------------------------*/
/* USER CODE BEGIN PD */

/* USER CODE END PD */

/* Private macro -------------------------------------------------------------*/
/* USER CODE BEGIN PM */

/* USER CODE END PM */

/* Private variables ---------------------------------------------------------*/

/* USER CODE BEGIN PV */

/* ---------------------------------------------------------------------------
 * Buffer sizing — all values derived from DFSDM output rate = 32 000 Hz
 *
 *   AUDIO_BUFFER_SIZE  : total circular DMA buffer (int16_t samples)
 *   AUDIO_HALF_SIZE    : samples delivered per DMA half/full callback
 *   UART_TX_PKT_SIZE   : bytes in one UART burst  = 4-byte sync + audio bytes
 *
 * At 512 total / 256 per half:
 *   fill window  = 256 / 32 000  = 8.0 ms
 *   TX time      = 516 B × 10 b / 921 600 baud ≈ 5.6 ms
 *   margin       = 8.0 − 5.6    = 2.4 ms  ← comfortable for DMA-to-DMA
 *
 * To match quara-firmware exactly (64 total / 32 per half = 1 ms window,
 * 0.74 ms TX, 0.26 ms margin) change AUDIO_BUFFER_SIZE to 64.
 * ---------------------------------------------------------------------------
 */
#define AUDIO_BUFFER_SIZE   512U
#define AUDIO_HALF_SIZE     (AUDIO_BUFFER_SIZE / 2U)                /* 256 samples  */
#define UART_TX_PKT_SIZE    (4U + AUDIO_HALF_SIZE * sizeof(int16_t))/* 516 bytes    */

/* DFSDM circular DMA buffer — must live in AXI SRAM (D1 domain, DMA-reachable).
 * DTCMRAM (.bss default) is CPU-only and unreachable by DMA bus masters.        */
__attribute__((section(".DMA_Buffer"))) int16_t audioBuffer[AUDIO_BUFFER_SIZE];

/* Ping-pong UART TX packets, also in AXI SRAM so UART4 TX DMA can read them.
 * Layout per packet: [0xAA,0x55,0xAA,0x55][256 × int16_t in little-endian]
 * The sync header is written once at init; audio bytes are overwritten every
 * callback by a ~0.4 µs memcpy before the UART TX DMA is kicked off.           */
__attribute__((section(".DMA_Buffer"))) static uint8_t uart_tx_ping[UART_TX_PKT_SIZE];
__attribute__((section(".DMA_Buffer"))) static uint8_t uart_tx_pong[UART_TX_PKT_SIZE];

/* USER CODE END PV */

/* Private function prototypes -----------------------------------------------*/
void SystemClock_Config(void);
void PeriphCommonClock_Config(void);
static void MPU_Config(void);
/* USER CODE BEGIN PFP */

/* USER CODE END PFP */

/* Private user code ---------------------------------------------------------*/
/* USER CODE BEGIN 0 */
extern UART_HandleTypeDef huart4;
extern DFSDM_Filter_HandleTypeDef hdfsdm1_filter1;

/* ---------------------------------------------------------------------------
 * DMA-to-DMA audio pipeline
 *
 *  1. DFSDM → DMA1_Stream0 fills audioBuffer[512] in circular mode (int16_t).
 *  2. On each half/full DMA callback (every 8 ms @ 32 kHz):
 *       a. memcpy the stable half into the correct ping or pong TX packet
 *          (audio bytes start at offset 4, after the pre-filled sync header).
 *       b. HAL_UART_Transmit_DMA kicks off DMA1_Stream1 to stream the 516-byte
 *          packet to UART4 — returns immediately, CPU stays free.
 *  3. The two DMA engines run concurrently: DFSDM fills the OTHER half while
 *     UART ships the completed half.
 *
 *  Timing (32 kHz, 921 600 baud, 8N1):
 *    half-buffer fill  = 256 / 32 000 = 8.0 ms
 *    UART TX           = 516 × 10 / 921 600 ≈ 5.6 ms
 *    margin            = 2.4 ms
 * ---------------------------------------------------------------------------
 */

/* Called from DMA1_Stream0 ISR when DFSDM DMA has filled the FIRST half.
 * audioBuffer[0 .. AUDIO_HALF_SIZE-1] is now stable.                       */
void HAL_DFSDM_FilterRegConvHalfCpltCallback(DFSDM_Filter_HandleTypeDef *hdfsdm_filter)
{
  if (hdfsdm_filter == &hdfsdm1_filter1)
  {
    /* Stage audio into ping packet (sync header already in place at [0..3]) */
    memcpy(&uart_tx_ping[4], &audioBuffer[0],
           AUDIO_HALF_SIZE * sizeof(int16_t));

    /* Launch UART TX DMA — non-blocking, returns immediately.
     * If the previous TX is somehow still in progress HAL returns HAL_BUSY
     * and we silently drop this burst (should not happen with 2.4 ms margin). */
    (void)HAL_UART_Transmit_DMA(&huart4, uart_tx_ping,
                                 (uint16_t)UART_TX_PKT_SIZE);
  }
}

/* Called from DMA1_Stream0 ISR when DFSDM DMA has filled the SECOND half.
 * audioBuffer[AUDIO_HALF_SIZE .. AUDIO_BUFFER_SIZE-1] is now stable.       */
void HAL_DFSDM_FilterRegConvCpltCallback(DFSDM_Filter_HandleTypeDef *hdfsdm_filter)
{
  if (hdfsdm_filter == &hdfsdm1_filter1)
  {
    memcpy(&uart_tx_pong[4], &audioBuffer[AUDIO_HALF_SIZE],
           AUDIO_HALF_SIZE * sizeof(int16_t));

    (void)HAL_UART_Transmit_DMA(&huart4, uart_tx_pong,
                                 (uint16_t)UART_TX_PKT_SIZE);
  }
}
/* USER CODE END 0 */

/**
  * @brief  The application entry point.
  * @retval int
  */
int main(void)
{

  /* USER CODE BEGIN 1 */

  /* USER CODE END 1 */

  /* MPU Configuration--------------------------------------------------------*/
  MPU_Config();

  /* MCU Configuration--------------------------------------------------------*/

  /* Reset of all peripherals, Initializes the Flash interface and the Systick. */
  HAL_Init();

  /* USER CODE BEGIN Init */

  /* USER CODE END Init */

  /* Configure the system clock */
  SystemClock_Config();

  /* Configure the peripherals common clocks */
  PeriphCommonClock_Config();

  /* USER CODE BEGIN SysInit */

  /* USER CODE END SysInit */

  /* Initialize all configured peripherals */
  MX_GPIO_Init();
  MX_DMA_Init();
  MX_DFSDM1_Init();
  MX_UART4_Init();
  /* USER CODE BEGIN 2 */
  /* Pre-fill the 4-byte sync header in both TX packets once at startup.
   * The pattern 0xAA 0x55 0xAA 0x55 lets the host detect and re-align
   * frame boundaries robustly (alternating bit pattern, not a valid int16). */
  uart_tx_ping[0] = uart_tx_pong[0] = 0xAAU;
  uart_tx_ping[1] = uart_tx_pong[1] = 0x55U;
  uart_tx_ping[2] = uart_tx_pong[2] = 0xAAU;
  uart_tx_ping[3] = uart_tx_pong[3] = 0x55U;

  /* Startup banner — confirms UART4 is alive before first audio packet */
  static const char banner[] = "DFSDM audio ready\r\n";
  HAL_UART_Transmit(&huart4, (uint8_t*)banner, sizeof(banner) - 1, HAL_MAX_DELAY);

  /* Start DFSDM circular DMA.  Half/full callbacks will fire automatically
   * every 8 ms and chain the UART TX DMA without further CPU involvement.  */
  if (HAL_DFSDM_FilterRegularMsbStart_DMA(&hdfsdm1_filter1,
                                           audioBuffer,
                                           AUDIO_BUFFER_SIZE) != HAL_OK)
  {
    Error_Handler();
  }
  /* USER CODE END 2 */

  /* Infinite loop */
  /* USER CODE BEGIN WHILE */
  while (1)
  {
    /* USER CODE END WHILE */

    /* USER CODE BEGIN 3 */
    /* All data flow is DMA-driven.
     * DFSDM DMA fills audioBuffer; half/full callbacks fire every 8 ms and
     * immediately chain UART4 TX DMA — CPU stays idle here.               */
  }
  /* USER CODE END 3 */
}

/**
  * @brief System Clock Configuration
  * @retval None
  */
void SystemClock_Config(void)
{
  RCC_OscInitTypeDef RCC_OscInitStruct = {0};
  RCC_ClkInitTypeDef RCC_ClkInitStruct = {0};

  /* Supply: Direct SMPS */
  HAL_PWREx_ConfigSupply(PWR_DIRECT_SMPS_SUPPLY);

  /* VOS2: supports up to 300 MHz SYSCLK / 150 MHz AHB */
  __HAL_PWR_VOLTAGESCALING_CONFIG(PWR_REGULATOR_VOLTAGE_SCALE2);

  while (!__HAL_PWR_GET_FLAG(PWR_FLAG_VOSRDY)) {}

  /* HSE on, HSI on (default), PLL1 from HSE
   * PLL1: M=24 -> VCO_in=1MHz (VCIRANGE_0), N=288 -> VCO=288MHz (VCOMEDIUM),
   *        P=1 -> SYSCLK=288MHz, Q=6 -> 48MHz, R=6 -> 48MHz */
  RCC_OscInitStruct.OscillatorType = RCC_OSCILLATORTYPE_HSI | RCC_OSCILLATORTYPE_HSE;
  RCC_OscInitStruct.HSEState = RCC_HSE_ON;
  RCC_OscInitStruct.HSIState = RCC_HSI_DIV1;
  RCC_OscInitStruct.HSICalibrationValue = 64;
  RCC_OscInitStruct.PLL.PLLState = RCC_PLL_ON;
  RCC_OscInitStruct.PLL.PLLSource = RCC_PLLSOURCE_HSE;
  RCC_OscInitStruct.PLL.PLLM = 24;
  RCC_OscInitStruct.PLL.PLLN = 288;
  RCC_OscInitStruct.PLL.PLLP = 1;
  RCC_OscInitStruct.PLL.PLLQ = 6;
  RCC_OscInitStruct.PLL.PLLR = 6;
  RCC_OscInitStruct.PLL.PLLRGE = RCC_PLL1VCIRANGE_0;
  RCC_OscInitStruct.PLL.PLLVCOSEL = RCC_PLL1VCOMEDIUM;
  RCC_OscInitStruct.PLL.PLLFRACN = 0;
  if (HAL_RCC_OscConfig(&RCC_OscInitStruct) != HAL_OK)
  {
    Error_Handler();
  }

  /* SYSCLK=PLL1_P=288MHz, D1CPRE=/1 (CPU=288MHz), HPRE=/2 (AXI/AHB=144MHz),
   * APBx prescalers /2 -> 72MHz; FLASH_LATENCY_2 for AXI=144MHz at VOS2 */
  RCC_ClkInitStruct.ClockType = RCC_CLOCKTYPE_HCLK | RCC_CLOCKTYPE_SYSCLK
                              | RCC_CLOCKTYPE_PCLK1 | RCC_CLOCKTYPE_PCLK2
                              | RCC_CLOCKTYPE_D3PCLK1 | RCC_CLOCKTYPE_D1PCLK1;
  RCC_ClkInitStruct.SYSCLKSource = RCC_SYSCLKSOURCE_PLLCLK;
  RCC_ClkInitStruct.SYSCLKDivider = RCC_SYSCLK_DIV1;
  RCC_ClkInitStruct.AHBCLKDivider = RCC_HCLK_DIV2;
  RCC_ClkInitStruct.APB3CLKDivider = RCC_APB3_DIV2;
  RCC_ClkInitStruct.APB1CLKDivider = RCC_APB1_DIV2;
  RCC_ClkInitStruct.APB2CLKDivider = RCC_APB2_DIV2;
  RCC_ClkInitStruct.APB4CLKDivider = RCC_APB4_DIV2;

  if (HAL_RCC_ClockConfig(&RCC_ClkInitStruct, FLASH_LATENCY_2) != HAL_OK)
  {
    Error_Handler();
  }
  HAL_RCC_MCOConfig(RCC_MCO1, RCC_MCO1SOURCE_HSE, RCC_MCODIV_1);
}

/**
  * @brief Peripherals Common Clock Configuration
  * @retval None
  */
void PeriphCommonClock_Config(void)
{
  RCC_PeriphCLKInitTypeDef PeriphClkInitStruct = {0};

  /* PLL3: M=24 -> VCO_in=1MHz (VCIRANGE_0), N=320 -> VCO=320MHz (VCOMEDIUM),
   *        P=10 -> PLL3_P=32MHz (DFSDM audio CKOUT via SAI1 source),
   *        Q=4  -> PLL3_Q=80MHz, R=16 -> PLL3_R=20MHz */
  PeriphClkInitStruct.PeriphClockSelection = RCC_PERIPHCLK_SAI1;
  PeriphClkInitStruct.Sai1ClockSelection = RCC_SAI1CLKSOURCE_PLL3;
  PeriphClkInitStruct.PLL3.PLL3M = 24;
  PeriphClkInitStruct.PLL3.PLL3N = 320;
  PeriphClkInitStruct.PLL3.PLL3P = 10;
  PeriphClkInitStruct.PLL3.PLL3Q = 4;
  PeriphClkInitStruct.PLL3.PLL3R = 16;
  PeriphClkInitStruct.PLL3.PLL3RGE = RCC_PLL3VCIRANGE_0;
  PeriphClkInitStruct.PLL3.PLL3VCOSEL = RCC_PLL3VCOMEDIUM;
  PeriphClkInitStruct.PLL3.PLL3FRACN = 0;
  if (HAL_RCCEx_PeriphCLKConfig(&PeriphClkInitStruct) != HAL_OK)
  {
    Error_Handler();
  }
}

/* USER CODE BEGIN 4 */

/* USER CODE END 4 */

 /* MPU Configuration */

void MPU_Config(void)
{
  MPU_Region_InitTypeDef MPU_InitStruct = {0};

  /* Disables the MPU */
  HAL_MPU_Disable();

  /** Initializes and configures the Region and the memory to be protected
  */
  MPU_InitStruct.Enable = MPU_REGION_ENABLE;
  MPU_InitStruct.Number = MPU_REGION_NUMBER0;
  MPU_InitStruct.BaseAddress = 0x0;
  MPU_InitStruct.Size = MPU_REGION_SIZE_4GB;
  MPU_InitStruct.SubRegionDisable = 0x87;
  MPU_InitStruct.TypeExtField = MPU_TEX_LEVEL0;
  MPU_InitStruct.AccessPermission = MPU_REGION_NO_ACCESS;
  MPU_InitStruct.DisableExec = MPU_INSTRUCTION_ACCESS_DISABLE;
  MPU_InitStruct.IsShareable = MPU_ACCESS_SHAREABLE;
  MPU_InitStruct.IsCacheable = MPU_ACCESS_NOT_CACHEABLE;
  MPU_InitStruct.IsBufferable = MPU_ACCESS_NOT_BUFFERABLE;

  HAL_MPU_ConfigRegion(&MPU_InitStruct);
  /* Enables the MPU */
  HAL_MPU_Enable(MPU_PRIVILEGED_DEFAULT);

}

/**
  * @brief  This function is executed in case of error occurrence.
  * @retval None
  */
void Error_Handler(void)
{
  /* USER CODE BEGIN Error_Handler_Debug */
  /* User can add his own implementation to report the HAL error return state */
  __disable_irq();
  while (1)
  {
  }
  /* USER CODE END Error_Handler_Debug */
}
#ifdef USE_FULL_ASSERT
/**
  * @brief  Reports the name of the source file and the source line number
  *         where the assert_param error has occurred.
  * @param  file: pointer to the source file name
  * @param  line: assert_param error line source number
  * @retval None
  */
void assert_failed(uint8_t *file, uint32_t line)
{
  /* USER CODE BEGIN 6 */
  /* User can add his own implementation to report the file name and line number,
     ex: printf("Wrong parameters value: file %s on line %d\r\n", file, line) */
  /* USER CODE END 6 */
}
#endif /* USE_FULL_ASSERT */
