(defglobal
  ?*fields* = (create$ red "two words" "a\"b" "a\\b" "" "42" "[widget]" [widget] [a?b] 3 1.25 -0.0)
  ?*encoded* = FALSE
  ?*restored* = (create$))

(defrule probe =>
  (bind ?*encoded* (implode$ ?*fields*))
  (bind ?*restored* (explode$ ?*encoded*))
  (printout t
    (eq ?*fields* ?*restored*) "|"
    (length$ ?*restored*) "|"
    (stringp (nth$ 7 ?*restored*)) "|"
    (instance-namep (nth$ 8 ?*restored*)) "|"
    (instance-namep (nth$ 9 ?*restored*)) "|"
    (integerp (nth$ 10 ?*restored*)) "|"
    (floatp (nth$ 11 ?*restored*)) "|"
    (floatp (nth$ 12 ?*restored*)) crlf)
  (printout t ?*encoded* crlf))
