; Harness for telefonica-clips/branches/64x/test_suite/gnrcovl.clp
; Detected constructs: defglobal: ?*success*; deffunction: alt-str-cat/1, print-result/2, testit/0; defmethod: sym-cat
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-2778a06b3d7816461c9187b39d95ea678d413b9e67befc1d8d04501330323704-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-2778a06b3d7816461c9187b39d95ea678d413b9e67befc1d8d04501330323704|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-2778a06b3d7816461c9187b39d95ea678d413b9e67befc1d8d04501330323704|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-2778a06b3d7816461c9187b39d95ea678d413b9e67befc1d8d04501330323704|COMPLETE" crlf))
