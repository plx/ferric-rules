(deffunction bad (?x) (if ?x then (missing-call ?x) else 0))
