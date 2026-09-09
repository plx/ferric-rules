(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (multifieldp ?value) ":"
  (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show flattened (member$ (create$ (create$ b) c) (create$ a (create$ b c) d)))
 (show flattened-empty (member$ (create$ (create$)) (create$ a b)))
 (show generated-symbol (member$ (create$ (sym-cat "red") "blue") (create$ red "blue")))
)
