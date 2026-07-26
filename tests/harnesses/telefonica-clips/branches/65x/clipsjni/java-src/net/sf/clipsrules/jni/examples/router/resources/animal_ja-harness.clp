; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/animal_ja.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-54dbb87cb5894b4f4008b84976bc621c07dc40c46787ef8d3072a1f4ef962348-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-54dbb87cb5894b4f4008b84976bc621c07dc40c46787ef8d3072a1f4ef962348|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-54dbb87cb5894b4f4008b84976bc621c07dc40c46787ef8d3072a1f4ef962348|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-54dbb87cb5894b4f4008b84976bc621c07dc40c46787ef8d3072a1f4ef962348|COMPLETE" crlf))
