/* USER CODE BEGIN Header */
/**
  ******************************************************************************
  * @file    usart.c
  * @brief   This file provides code for the configuration
  *          of the USART instances.
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
#include "usart.h"

/* USER CODE BEGIN 0 */

/* USER CODE END 0 */

UART_HandleTypeDef huart4;
/* UART4 TX DMA handle — DMA1_Stream1, DMA_REQUEST_UART4_TX.
 * Declared here so both usart.c and stm32h7xx_it.c (via extern) can reach it. */
DMA_HandleTypeDef hdma_uart4_tx;

/* UART4 init function */
void MX_UART4_Init(void)
{

  /* USER CODE BEGIN UART4_Init 0 */

  /* USER CODE END UART4_Init 0 */

  /* USER CODE BEGIN UART4_Init 1 */

  /* USER CODE END UART4_Init 1 */
  huart4.Instance = UART4;
  huart4.Init.BaudRate = 921600;
  huart4.Init.WordLength = UART_WORDLENGTH_8B;
  huart4.Init.StopBits = UART_STOPBITS_1;
  huart4.Init.Parity = UART_PARITY_NONE;
  huart4.Init.Mode = UART_MODE_TX_RX;
  huart4.Init.HwFlowCtl = UART_HWCONTROL_NONE;
  huart4.Init.OverSampling = UART_OVERSAMPLING_16;
  huart4.Init.OneBitSampling = UART_ONE_BIT_SAMPLE_DISABLE;
  huart4.Init.ClockPrescaler = UART_PRESCALER_DIV1;
  huart4.AdvancedInit.AdvFeatureInit = UART_ADVFEATURE_NO_INIT;
  if (HAL_UART_Init(&huart4) != HAL_OK)
  {
    Error_Handler();
  }
  if (HAL_UARTEx_SetTxFifoThreshold(&huart4, UART_TXFIFO_THRESHOLD_1_8) != HAL_OK)
  {
    Error_Handler();
  }
  if (HAL_UARTEx_SetRxFifoThreshold(&huart4, UART_RXFIFO_THRESHOLD_1_8) != HAL_OK)
  {
    Error_Handler();
  }
  if (HAL_UARTEx_DisableFifoMode(&huart4) != HAL_OK)
  {
    Error_Handler();
  }
  /* USER CODE BEGIN UART4_Init 2 */

  /* USER CODE END UART4_Init 2 */

}

void HAL_UART_MspInit(UART_HandleTypeDef* uartHandle)
{

  GPIO_InitTypeDef GPIO_InitStruct = {0};
  RCC_PeriphCLKInitTypeDef PeriphClkInitStruct = {0};
  if(uartHandle->Instance==UART4)
  {
  /* USER CODE BEGIN UART4_MspInit 0 */

  /* USER CODE END UART4_MspInit 0 */

  /** Initializes the peripherals clock
  */
    PeriphClkInitStruct.PeriphClockSelection = RCC_PERIPHCLK_UART4;
    PeriphClkInitStruct.Usart234578ClockSelection = RCC_USART234578CLKSOURCE_D2PCLK1;
    if (HAL_RCCEx_PeriphCLKConfig(&PeriphClkInitStruct) != HAL_OK)
    {
      Error_Handler();
    }

    /* UART4 clock enable */
    __HAL_RCC_UART4_CLK_ENABLE();

    __HAL_RCC_GPIOH_CLK_ENABLE();
    /**UART4 GPIO Configuration
    PH14     ------> UART4_RX
    PH13     ------> UART4_TX
    */
    GPIO_InitStruct.Pin = GPIO_PIN_14|GPIO_PIN_13;
    GPIO_InitStruct.Mode = GPIO_MODE_AF_PP;
    GPIO_InitStruct.Pull = GPIO_NOPULL;
    GPIO_InitStruct.Speed = GPIO_SPEED_FREQ_LOW;
    GPIO_InitStruct.Alternate = GPIO_AF8_UART4;
    HAL_GPIO_Init(GPIOH, &GPIO_InitStruct);

    /* -----------------------------------------------------------------
     * UART4 TX DMA — DMA1_Stream1, request UART4_TX
     *
     * Memory→Peripheral, normal (one-shot) mode, byte alignment.
     * Priority LOW keeps it below the DFSDM DMA (Stream0, priority 0)
     * so audio capture is never delayed by an in-progress UART burst.
     * ----------------------------------------------------------------- */
    hdma_uart4_tx.Instance                 = DMA1_Stream1;
    hdma_uart4_tx.Init.Request             = DMA_REQUEST_UART4_TX;
    hdma_uart4_tx.Init.Direction           = DMA_MEMORY_TO_PERIPH;
    hdma_uart4_tx.Init.PeriphInc           = DMA_PINC_DISABLE;
    hdma_uart4_tx.Init.MemInc              = DMA_MINC_ENABLE;
    hdma_uart4_tx.Init.PeriphDataAlignment = DMA_PDATAALIGN_BYTE;
    hdma_uart4_tx.Init.MemDataAlignment    = DMA_MDATAALIGN_BYTE;
    hdma_uart4_tx.Init.Mode                = DMA_NORMAL;       /* one burst per packet */
    hdma_uart4_tx.Init.Priority            = DMA_PRIORITY_LOW;
    hdma_uart4_tx.Init.FIFOMode            = DMA_FIFOMODE_DISABLE;
    if (HAL_DMA_Init(&hdma_uart4_tx) != HAL_OK)
    {
      Error_Handler();
    }
    /* Link the DMA handle to the UART TX path */
    __HAL_LINKDMA(uartHandle, hdmatx, hdma_uart4_tx);

  /* USER CODE BEGIN UART4_MspInit 1 */
    /* UART4 global IRQ — needed so the HAL state machine can reset
     * gState to READY after each DMA TX via UART_EndTransmit_IT.
     * Without this, gState stays BUSY_TX after the first packet and
     * every subsequent HAL_UART_Transmit_DMA returns HAL_BUSY.
     * Priority 2: below DFSDM DMA (0) and UART TX DMA (1).          */
    HAL_NVIC_SetPriority(UART4_IRQn, 2, 0);
    HAL_NVIC_EnableIRQ(UART4_IRQn);
  /* USER CODE END UART4_MspInit 1 */
  }
}

void HAL_UART_MspDeInit(UART_HandleTypeDef* uartHandle)
{

  if(uartHandle->Instance==UART4)
  {
  /* USER CODE BEGIN UART4_MspDeInit 0 */

  /* USER CODE END UART4_MspDeInit 0 */
    /* Peripheral clock disable */
    __HAL_RCC_UART4_CLK_DISABLE();

    /**UART4 GPIO Configuration
    PH14     ------> UART4_RX
    PH13     ------> UART4_TX
    */
    HAL_GPIO_DeInit(GPIOH, GPIO_PIN_14|GPIO_PIN_13);

    /* De-init the TX DMA stream */
    HAL_DMA_DeInit(uartHandle->hdmatx);

  /* USER CODE BEGIN UART4_MspDeInit 1 */
    HAL_NVIC_DisableIRQ(UART4_IRQn);
  /* USER CODE END UART4_MspDeInit 1 */
  }
}

/* USER CODE BEGIN 1 */

/* USER CODE END 1 */

