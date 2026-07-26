; Harness for telefonica-clips/branches/64x/windows/MVS_2015/RouterWPFExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-67a00deafb122f64a3e4a929859188cc91c55fd427d61d37f14061b1e121df17-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-67a00deafb122f64a3e4a929859188cc91c55fd427d61d37f14061b1e121df17|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-67a00deafb122f64a3e4a929859188cc91c55fd427d61d37f14061b1e121df17|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-67a00deafb122f64a3e4a929859188cc91c55fd427d61d37f14061b1e121df17|COMPLETE" crlf))
