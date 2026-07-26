; Harness for telefonica-clips/branches/63x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_ru.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-c93c1b1f3adefed51e9a81140e5a4f503b861f803cdcdf44cc14c80b802137b2-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-c93c1b1f3adefed51e9a81140e5a4f503b861f803cdcdf44cc14c80b802137b2|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-c93c1b1f3adefed51e9a81140e5a4f503b861f803cdcdf44cc14c80b802137b2|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-c93c1b1f3adefed51e9a81140e5a4f503b861f803cdcdf44cc14c80b802137b2|COMPLETE" crlf))
