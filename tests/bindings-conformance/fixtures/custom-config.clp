(deffunction recurse (?remaining)
    (if (= ?remaining 0) then
        (return 0))
    (return (recurse (- ?remaining 1))))

(defrule exercise-depth
    =>
    (recurse 2))
