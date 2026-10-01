(defglobal ?*trace* = 0)
(deffunction mark (?digit ?value) (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
(defrule probe =>
 (printout t "[" (sub-string (mark 1 0) (mark 2 2) (mark 3 "abc")) "]:" ?*trace* crlf))
