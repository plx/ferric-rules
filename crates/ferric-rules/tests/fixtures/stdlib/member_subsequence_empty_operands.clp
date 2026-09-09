(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (multifieldp ?value) ":"
  (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show scalar-empty (member$ a (create$)))
 (show singleton-empty (member$ (create$ a) (create$)))
 (show empty-empty (member$ (create$) (create$)))
 (show empty-nonempty (member$ (create$) (create$ a b)))
)
