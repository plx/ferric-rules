; Ordered facts preserve integer, float, symbol and string values.
;; Level: basic
;; Covers: facts, ordered-scalar-types
; Protocol: load, reset, run to quiescence.
(deffacts input (sample 7 2.5 blue "hello world"))
(defrule observe
  (sample ?integer ?float ?symbol ?string)
  =>
  (printout t (integerp ?integer) " " (floatp ?float) " "
    (symbolp ?symbol) " " (stringp ?string) crlf))
