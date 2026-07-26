; Harness for telefonica-clips/branches/64x/windows/MVS_2015/AnimalWPFExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-a53bc85f804076a2a8cbed1cfee355e746eacfe9f2a2b828c57d64e82756b491-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-a53bc85f804076a2a8cbed1cfee355e746eacfe9f2a2b828c57d64e82756b491|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-a53bc85f804076a2a8cbed1cfee355e746eacfe9f2a2b828c57d64e82756b491|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-a53bc85f804076a2a8cbed1cfee355e746eacfe9f2a2b828c57d64e82756b491|COMPLETE" crlf))
