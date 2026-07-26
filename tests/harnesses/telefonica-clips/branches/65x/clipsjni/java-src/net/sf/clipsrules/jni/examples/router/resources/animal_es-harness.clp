; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/animal_es.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-08f508fd040d24bb1c39e51623e7c543b2c573abc5e5986bce51b3953ca2aabd-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-08f508fd040d24bb1c39e51623e7c543b2c573abc5e5986bce51b3953ca2aabd|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-08f508fd040d24bb1c39e51623e7c543b2c573abc5e5986bce51b3953ca2aabd|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-08f508fd040d24bb1c39e51623e7c543b2c573abc5e5986bce51b3953ca2aabd|COMPLETE" crlf))
