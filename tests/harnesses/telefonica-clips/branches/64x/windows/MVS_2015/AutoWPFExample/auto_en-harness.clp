; Harness for telefonica-clips/branches/64x/windows/MVS_2015/AutoWPFExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-fffd713c1509b757b22adb6fd8484b3fca5adb7e34dd22991934d492485d0784-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-fffd713c1509b757b22adb6fd8484b3fca5adb7e34dd22991934d492485d0784|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-fffd713c1509b757b22adb6fd8484b3fca5adb7e34dd22991934d492485d0784|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-fffd713c1509b757b22adb6fd8484b3fca5adb7e34dd22991934d492485d0784|COMPLETE" crlf))
