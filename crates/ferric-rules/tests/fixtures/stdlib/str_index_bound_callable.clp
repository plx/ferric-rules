(deffunction locate (?needle ?text) (str-index ?needle ?text))
(deffacts input (search "" "abc"))
(defrule probe (search ?needle ?text) =>
 (printout t (str-index ?needle ?text) ":" (locate ?needle ?text) ":"
  (locate ana banana) ":" (integerp (locate "" "abc")) crlf))
