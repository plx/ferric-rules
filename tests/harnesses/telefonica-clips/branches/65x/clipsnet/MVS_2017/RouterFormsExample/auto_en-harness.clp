; Harness for telefonica-clips/branches/65x/clipsnet/MVS_2017/RouterFormsExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-bc2c093fdf8aa13405ab56128e1e21448f7f62c4eab21cffae3c24ee0ce01fa9-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-bc2c093fdf8aa13405ab56128e1e21448f7f62c4eab21cffae3c24ee0ce01fa9|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-bc2c093fdf8aa13405ab56128e1e21448f7f62c4eab21cffae3c24ee0ce01fa9|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-bc2c093fdf8aa13405ab56128e1e21448f7f62c4eab21cffae3c24ee0ce01fa9|COMPLETE" crlf))
