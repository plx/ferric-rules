(deffunction show (?label ?value)
 (printout t ?label ":" (integerp ?value) ":" (multifieldp ?value) ":"
  (symbolp ?value) ":[" ?value "]" crlf))
(defglobal ?*trace* = 0)
(deffunction mark (?digit ?value) (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
(deffunction fail (?digit) (bind ?*trace* (+ (* ?*trace* 10) ?digit)) (/ 1 0))
(defrule probe =>
 (show void (member$ (printout t "first" crlf) (mark 2 (create$ a))))
 (bind ?*trace* 999)
 (printout t "trace:" ?*trace* crlf)
)
