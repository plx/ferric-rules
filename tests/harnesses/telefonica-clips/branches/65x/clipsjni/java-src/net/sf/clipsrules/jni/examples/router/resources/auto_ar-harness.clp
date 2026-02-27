; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/auto_ar.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
