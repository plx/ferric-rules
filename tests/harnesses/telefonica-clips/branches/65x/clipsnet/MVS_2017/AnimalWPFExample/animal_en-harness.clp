; Harness for telefonica-clips/branches/65x/clipsnet/MVS_2017/AnimalWPFExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-a2f5611d2ef95f3a88f9ae305b56af576e6d24083c702930f7564ab2620ba773-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-a2f5611d2ef95f3a88f9ae305b56af576e6d24083c702930f7564ab2620ba773|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-a2f5611d2ef95f3a88f9ae305b56af576e6d24083c702930f7564ab2620ba773|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-a2f5611d2ef95f3a88f9ae305b56af576e6d24083c702930f7564ab2620ba773|COMPLETE" crlf))
