(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (multifieldp ?value) ":"
  (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show multiple (member$ (create$ b c) (create$ a b c b c)))
 (show overlap (member$ (create$ a a) (create$ a a a)))
 (show failed-prefix (member$ (create$ a a b) (create$ a a a b)))
 (show scalar-repeat (member$ a (create$ b a a)))
 (show singleton-repeat (member$ (create$ a) (create$ b a a)))
)
