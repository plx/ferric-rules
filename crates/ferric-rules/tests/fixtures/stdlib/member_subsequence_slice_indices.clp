(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (multifieldp ?value) ":"
  (symbolp ?value) ":[" ?value "]" crlf))
(defrule probe =>
 (show needle-view (member$ (subseq$ (create$ x b c y) 2 3) (create$ a b c d)))
 (show hay-view (member$ (create$ b c) (subseq$ (create$ x a b c d y) 2 5)))
 (show both-views (member$ (subseq$ (create$ x b c y) 2 3) (subseq$ (create$ x a b c d y) 2 5)))
 (show sliced-singleton (member$ (subseq$ (create$ x c y) 2 2) (subseq$ (create$ x a b c y) 2 4)))
)
