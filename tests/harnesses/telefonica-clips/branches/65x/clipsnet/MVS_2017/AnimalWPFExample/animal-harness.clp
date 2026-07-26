; Harness for telefonica-clips/branches/65x/clipsnet/MVS_2017/AnimalWPFExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-4d84ed0a338674df8defe3e451b3fb58a15c5b06790ab6d0c3a0a3af85a70130-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-4d84ed0a338674df8defe3e451b3fb58a15c5b06790ab6d0c3a0a3af85a70130|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-4d84ed0a338674df8defe3e451b3fb58a15c5b06790ab6d0c3a0a3af85a70130|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-4d84ed0a338674df8defe3e451b3fb58a15c5b06790ab6d0c3a0a3af85a70130|COMPLETE" crlf))
