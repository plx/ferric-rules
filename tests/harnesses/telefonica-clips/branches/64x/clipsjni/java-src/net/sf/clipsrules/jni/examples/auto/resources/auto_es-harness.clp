; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_es.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-b778f112ac4612f21eb1f8777aa59f7b040059bbf93d321b4dd7776d6b692683-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-b778f112ac4612f21eb1f8777aa59f7b040059bbf93d321b4dd7776d6b692683|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-b778f112ac4612f21eb1f8777aa59f7b040059bbf93d321b4dd7776d6b692683|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-b778f112ac4612f21eb1f8777aa59f7b040059bbf93d321b4dd7776d6b692683|COMPLETE" crlf))
