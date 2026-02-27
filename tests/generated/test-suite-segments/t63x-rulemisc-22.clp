(deffacts xy
   (SAD G 1 GX01 "XX")
   (SAD G 1 GCH 1 GCH03 "AA")
   (SAD G 2 GX01 "CN")
   (SAD G 2 GCH 1 GCH03 "AA")
   (SAD G 3 GX01 "XX")
   (SAD G 3 GCH 1 GCH03 "B00")
   (SAD G 4 GX01 "CN")
   (SAD G 4 GCH 1 GCH03 "B00"))
(defrule if_exists ""
   (SAD G ?ix1 GX01 ?var1)
   (and
      (test (eq ?var1 "CN"))
      (exists 
         (SAD G ?ix1 GCH ?ix2 GCH03 ?var2)
         (test (eq ?var2 "B00"))))
   =>)
